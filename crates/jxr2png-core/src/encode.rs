//! PNG output.

use std::borrow::Cow;
use std::io::Write;

use png::{BitDepth, ColorType, Compression, SrgbRenderingIntent};
use rayon::prelude::*;

use crate::color::{
    SCRGB_WHITE_NITS, ToneCurve, bt709_to_bt2020, pq_encode, srgb_encode, tone_map_sdr,
};
use crate::decode::ScRgbImage;
use crate::icc::pq_sdr_fallback_profile;
use crate::{HdrOptions, Result, SdrOptions};

/// Light-level statistics of an image, in nits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LightLevels {
    /// Brightest channel value of any pixel (MaxCLL).
    pub max: f32,
    /// Average of each pixel's brightest channel (MaxFALL).
    pub average: f32,
    /// 99.9th percentile of each pixel's brightest channel.
    pub p999: f32,
}

pub fn light_levels(image: &ScRgbImage) -> LightLevels {
    let mut levels: Vec<f32> = image
        .pixels
        .par_iter()
        .map(|p| p[0].max(p[1]).max(p[2]).max(0.0) * SCRGB_WHITE_NITS)
        .collect();
    if levels.is_empty() {
        return LightLevels {
            max: 0.0,
            average: 0.0,
            p999: 0.0,
        };
    }
    let average =
        (levels.par_iter().map(|&v| f64::from(v)).sum::<f64>() / levels.len() as f64) as f32;
    let max = levels.par_iter().copied().reduce(|| 0.0, f32::max);
    let idx = ((levels.len() - 1) as f64 * 0.999) as usize;
    let (_, p999, _) = levels.select_nth_unstable_by(idx, f32::total_cmp);
    LightLevels {
        max,
        average,
        p999: *p999,
    }
}

/// Write a 16-bit PNG in BT.2020 primaries with the PQ transfer function.
///
/// Highlights above `hdr.peak_nits` roll off smoothly so the brightest
/// pixel lands on that peak. `sdr` sets the tone curve of the embedded ICC
/// profile, which viewers without HDR support use instead of its `cicp`
/// tag.
///
/// HDR is signalled only by the profile's `cicp` tag, not by a PNG `cICP`
/// chunk. Chrome, Edge and Windows Photos read the tag, while Discord's
/// image service converts any PNG with a `cICP` chunk to SDR with its red
/// and blue channels mixed up, which turns the picture yellow. Without the
/// chunk it keeps the profile and the image shows correctly.
pub fn write_hdr_png<W: Write>(
    image: &ScRgbImage,
    levels: &LightLevels,
    hdr: &HdrOptions,
    sdr: &SdrOptions,
    out: W,
) -> Result<()> {
    // Work in BT.2020 nits; take each pixel's brightest channel for the
    // roll-off so hue is kept, as in the SDR path.
    let to_nits = |p: [f32; 3]| bt709_to_bt2020(p).map(|c| c.max(0.0) * SCRGB_WHITE_NITS);
    let source_peak = image
        .pixels
        .par_iter()
        .map(|&p| to_nits(p).into_iter().fold(0.0, f32::max))
        .reduce(|| 0.0, f32::max);
    let roll_off = hdr
        .peak_nits
        .filter(|&peak| source_peak > peak)
        .map(|peak| (peak, ToneCurve::new(source_peak / peak)));

    let mut data = vec![0u8; image.pixels.len() * 6];
    let (max, sum) = data
        .par_chunks_mut(6)
        .zip(image.pixels.par_iter())
        .map(|(out, &p)| {
            let mut rgb = to_nits(p);
            let brightest = rgb.into_iter().fold(0.0, f32::max);
            if let Some((peak, curve)) = &roll_off
                && brightest > 0.0
            {
                let scale = curve.apply(brightest / peak) * peak / brightest;
                rgb = rgb.map(|c| c * scale);
            }
            for (i, c) in rgb.into_iter().enumerate() {
                let v = quantize_pq(pq_encode(c));
                out[i * 2..i * 2 + 2].copy_from_slice(&v.to_be_bytes());
            }
            let brightest = rgb.into_iter().fold(0.0, f32::max);
            (brightest, f64::from(brightest))
        })
        .reduce(|| (0.0, 0.0), |a, b| (a.0.max(b.0), a.1 + b.1));
    let average = (sum / image.pixels.len().max(1) as f64) as f32;

    let mut info = png::Info::with_size(image.width, image.height);
    info.color_type = ColorType::Rgb;
    info.bit_depth = BitDepth::Sixteen;
    info.icc_profile = Some(Cow::Owned(pq_sdr_fallback_profile(
        sdr.white_nits,
        sdr_peak_nits(levels, sdr),
    )));
    let mut encoder = png::Encoder::with_info(out, info)?;
    // 16-bit PQ data compresses poorly with `Fast`; `Balanced` makes the
    // file about a third smaller at no loss. The 8-bit SDR PNG gains little.
    encoder.set_compression(Compression::Balanced);
    let mut writer = encoder.write_header()?;

    // sBIT: the samples carry only this many significant bits.
    let bits = HDR_SIGNIFICANT_BITS as u8;
    writer.write_chunk(png::chunk::sBIT, &[bits; 3])?;
    // mDCV: BT.2020 primaries and D65 white in units of 0.00002, then the
    // display's peak and minimum luminance in units of 0.0001 nits.
    let mastering_peak = roll_off.map_or(max, |(peak, _)| peak);
    let mut mdcv = Vec::with_capacity(24);
    for xy in [0.708, 0.292, 0.170, 0.797, 0.131, 0.046, 0.3127, 0.3290] {
        mdcv.extend_from_slice(&((xy * 50_000.0f64).round() as u16).to_be_bytes());
    }
    mdcv.extend_from_slice(&to_clli_units(mastering_peak).to_be_bytes());
    mdcv.extend_from_slice(&to_clli_units(0.005).to_be_bytes());
    writer.write_chunk(png::chunk::ChunkType(*b"mDCV"), &mdcv)?;
    // cLLI: MaxCLL and MaxFALL in units of 0.0001 nits.
    let mut clli = [0u8; 8];
    clli[..4].copy_from_slice(&to_clli_units(max).to_be_bytes());
    clli[4..].copy_from_slice(&to_clli_units(average).to_be_bytes());
    writer.write_chunk(png::chunk::ChunkType(*b"cLLI"), &clli)?;

    writer.write_image_data(&data)?;
    writer.finish()?;
    Ok(())
}

/// Bits of PQ precision kept in the 16-bit HDR samples. Ten bits is what
/// HDR10 displays and video use; dropping the bits below it makes the PNG
/// about half the size.
const HDR_SIGNIFICANT_BITS: u32 = 10;

/// Round a 0..1 PQ signal to [`HDR_SIGNIFICANT_BITS`] and store it as a
/// 16-bit sample. The steps are multiples of 64 on the 16-bit scale, so
/// viewers that read the sample as `v / 65535` (most ignore `sBIT`) get the
/// signal to within half a step, with no overall brightness shift.
fn quantize_pq(signal: f32) -> u16 {
    let step = 1u32 << (16 - HDR_SIGNIFICANT_BITS);
    let max_code = (1u32 << HDR_SIGNIFICANT_BITS) - 1;
    let code = ((signal * 65535.0 / step as f32).round() as u32).min(max_code);
    (code * step) as u16
}

/// Brightness that maps to SDR maximum.
fn sdr_peak_nits(levels: &LightLevels, options: &SdrOptions) -> f32 {
    options
        .peak_nits
        .unwrap_or(levels.p999)
        .max(options.white_nits)
}

fn to_clli_units(nits: f32) -> u32 {
    (f64::from(nits.clamp(0.0, 10_000.0)) * 10_000.0).round() as u32
}

/// Tone-map to 8-bit sRGB, as packed RGB rows: the pixels of the SDR PNG.
pub fn sdr_pixels(image: &ScRgbImage, levels: &LightLevels, options: &SdrOptions) -> Vec<u8> {
    let white = options.white_nits / SCRGB_WHITE_NITS;
    let curve = ToneCurve::new(sdr_peak_nits(levels, options) / options.white_nits);
    let mut data = vec![0u8; image.pixels.len() * 3];
    data.par_chunks_mut(3)
        .zip(image.pixels.par_iter())
        .for_each(|(out, &p)| {
            for (o, c) in out.iter_mut().zip(tone_map_sdr(p, white, &curve)) {
                *o = (srgb_encode(c) * 255.0).round() as u8;
            }
        });
    data
}

/// Write a tone-mapped 8-bit sRGB PNG.
pub fn write_sdr_png<W: Write>(
    image: &ScRgbImage,
    levels: &LightLevels,
    options: &SdrOptions,
    out: W,
) -> Result<()> {
    let data = sdr_pixels(image, levels, options);
    let mut encoder = png::Encoder::new(out, image.width, image.height);
    encoder.set_color(ColorType::Rgb);
    encoder.set_depth(BitDepth::Eight);
    encoder.set_compression(Compression::Fast);
    encoder.set_source_srgb(SrgbRenderingIntent::Perceptual);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(&data)?;
    writer.finish()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(pixels: Vec<[f32; 3]>, width: u32) -> ScRgbImage {
        let height = pixels.len() as u32 / width;
        ScRgbImage {
            width,
            height,
            pixels,
        }
    }

    fn read_png(bytes: &[u8]) -> (png::Info<'static>, Vec<u8>) {
        let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
            .read_info()
            .unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let frame = reader.next_frame(&mut buf).unwrap();
        buf.truncate(frame.buffer_size());
        (reader.info().clone(), buf)
    }

    #[test]
    fn light_levels_use_brightest_channel() {
        let img = image(vec![[1.0, 0.0, 0.0], [0.0, 0.0, 3.0]], 2);
        let levels = light_levels(&img);
        assert_eq!(levels.max, 240.0);
        assert_eq!(levels.average, 160.0);
    }

    #[test]
    fn hdr_png_is_tagged_and_pq_encoded() {
        // Black, scRGB white (80 nits) and 1000 nits white.
        let img = image(vec![[0.0; 3], [1.0; 3], [12.5; 3]], 3);
        let levels = light_levels(&img);
        let mut bytes = Vec::new();
        write_hdr_png(
            &img,
            &levels,
            &HdrOptions::default(),
            &SdrOptions::default(),
            &mut bytes,
        )
        .unwrap();

        let (info, data) = read_png(&bytes);
        assert!(info.icc_profile.is_some());
        assert_eq!(info.bit_depth, BitDepth::Sixteen);
        assert_eq!(info.color_type, ColorType::Rgb);
        // HDR is signalled by the profile's `cicp` tag only (see
        // `write_hdr_png`).
        assert!(info.coding_independent_code_points.is_none());
        let clli = info.content_light_level.unwrap();
        assert_eq!(clli.max_content_light_level, 10_000_000);

        assert_eq!(info.sbit.as_deref(), Some(&[10, 10, 10][..]));

        let sample = |i: usize| u16::from_be_bytes([data[i * 2], data[i * 2 + 1]]);
        assert_eq!(sample(0), 0);
        let expect = |nits: f32| (pq_encode(nits) * 65535.0).round() as u16;
        for c in 0..3 {
            assert!(sample(3 + c).abs_diff(expect(80.0)) <= 32);
            assert!(sample(6 + c).abs_diff(expect(1000.0)) <= 32);
        }
    }

    #[test]
    fn pq_is_rounded_to_ten_bits_on_the_16_bit_scale() {
        assert_eq!(quantize_pq(0.0), 0);
        assert_eq!(quantize_pq(1.0), 1023 << 6);
        for i in 0..=1000 {
            let signal = i as f32 / 1000.0;
            let v = quantize_pq(signal);
            assert_eq!(v % 64, 0);
            // Within half a 10-bit step, except at the very top where the
            // last step is cut off.
            assert!((f32::from(v) - signal * 65535.0).abs() <= 32.0 || v == 1023 << 6);
        }
    }

    #[test]
    fn hdr_highlights_roll_off_to_peak() {
        // 100 nits, 900 nits and a 4000 nit highlight, limited to 1000 nits.
        let img = image(vec![[1.25; 3], [11.25; 3], [50.0; 3]], 3);
        let levels = light_levels(&img);
        let mut bytes = Vec::new();
        write_hdr_png(
            &img,
            &levels,
            &HdrOptions {
                peak_nits: Some(1000.0),
            },
            &SdrOptions::default(),
            &mut bytes,
        )
        .unwrap();

        let (info, data) = read_png(&bytes);
        let nits = |i: usize| {
            crate::color::pq_decode(
                f32::from(u16::from_be_bytes([data[i * 2], data[i * 2 + 1]])) / 65535.0,
            )
        };
        assert!((nits(0) - 100.0).abs() < 1.0, "below the knee is unchanged");
        assert!(
            nits(3) > 750.0 && nits(3) < 900.0,
            "near the peak is compressed"
        );
        assert!(
            (nits(6) - 1000.0).abs() < 5.0,
            "brightest lands on the peak"
        );

        let clli = info.content_light_level.unwrap();
        assert!(clli.max_content_light_level.abs_diff(10_000_000) < 50_000);
        let mdcv = info.mastering_display_color_volume.unwrap();
        assert_eq!(mdcv.max_luminance, 10_000_000);
        assert_eq!(mdcv.chromaticities.red.0.into_scaled(), 70_800);
    }

    #[test]
    fn sdr_png_maps_reference_white_near_full_scale() {
        let options = SdrOptions {
            white_nits: 203.0,
            peak_nits: Some(2030.0),
        };
        let white = options.white_nits / SCRGB_WHITE_NITS;
        let img = image(vec![[0.0; 3], [white; 3], [white * 10.0; 3]], 3);
        let levels = light_levels(&img);
        let mut bytes = Vec::new();
        write_sdr_png(&img, &levels, &options, &mut bytes).unwrap();

        let (info, data) = read_png(&bytes);
        assert_eq!(info.bit_depth, BitDepth::Eight);
        assert!(info.srgb.is_some());
        assert_eq!(&data[0..3], &[0, 0, 0]);
        // Reference white sits just under the highlight roll-off.
        assert!(data[3] > 220 && data[3] < 255);
        assert_eq!(&data[6..9], &[255, 255, 255]);
    }
}
