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
/// profile, which viewers without HDR support use instead of the `cICP`
/// chunk.
///
/// HDR is signalled both by a PNG `cICP` chunk, which Windows Photos and
/// HDR image viewers need, and by the profile's `cicp` tag. Discord's
/// image service mishandles the chunk (see [`for_discord`]).
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

    let channels = if image.alpha.is_some() { 4 } else { 3 };
    let mut data = vec![0u8; image.pixels.len() * 2 * channels];
    let mut brightest: Vec<f32> = data
        .par_chunks_mut(2 * channels)
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
            rgb.into_iter().fold(0.0, f32::max)
        })
        .collect();
    if let Some(alpha) = &image.alpha {
        data.par_chunks_mut(8)
            .zip(alpha.par_iter())
            .for_each(|(out, &a)| {
                let a = (a.clamp(0.0, 1.0) * 65535.0).round() as u16;
                out[6..].copy_from_slice(&a.to_be_bytes());
            });
    }
    let max = brightest.par_iter().copied().reduce(|| 0.0, f32::max);
    let average = (brightest.par_iter().map(|&v| f64::from(v)).sum::<f64>()
        / brightest.len().max(1) as f64) as f32;
    let max_cll = max_content_light_level(&mut brightest);

    let mut info = png::Info::with_size(image.width, image.height);
    info.color_type = if image.alpha.is_some() {
        ColorType::Rgba
    } else {
        ColorType::Rgb
    };
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

    // cICP: BT.2020 primaries, PQ transfer, RGB (identity matrix), full range.
    writer.write_chunk(png::chunk::ChunkType(*b"cICP"), &[9, 16, 0, 1])?;
    // sBIT: the samples carry only this many significant bits.
    let bits = HDR_SIGNIFICANT_BITS as u8;
    let sbit = [bits, bits, bits, 16];
    writer.write_chunk(png::chunk::sBIT, &sbit[..channels])?;
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
    clli[..4].copy_from_slice(&to_clli_units(max_cll).to_be_bytes());
    clli[4..].copy_from_slice(&to_clli_units(average).to_be_bytes());
    writer.write_chunk(png::chunk::ChunkType(*b"cLLI"), &clli)?;

    writer.write_image_data(&data)?;
    writer.finish()?;
    Ok(())
}

/// MaxCLL for the per-pixel brightest channel values in `brightest`: the
/// 99.99th percentile rather than the true maximum. Following the PNG
/// spec's recommendation ("On the Calculation and Usage of HDR Static
/// Content Metadata", SMPTE), this ignores a handful of stray pixels, so
/// viewers don't dim the whole picture to fit a few specks. The mastering
/// peak in `mDCV` still covers the true maximum.
fn max_content_light_level(brightest: &mut [f32]) -> f32 {
    if brightest.is_empty() {
        return 0.0;
    }
    // Nearest-rank percentile: the smallest value at or above 99.99% of
    // the pixels.
    let rank = (brightest.len() as f64 * 0.9999).ceil() as usize;
    let (_, value, _) = brightest.select_nth_unstable_by(rank.max(1) - 1, f32::total_cmp);
    *value
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

/// Copy of an HDR PNG without its `cICP` chunk, for sharing on Discord.
///
/// Discord's image service converts any PNG with a `cICP` chunk to SDR with
/// its red and blue channels mixed up, which turns warm scenes yellow.
/// Without the chunk it keeps the ICC profile, whose `cicp` tag still marks
/// the image as HDR, and shows it correctly. The `mDCV` chunk goes too,
/// since the PNG spec only allows it alongside `cICP`. Every other chunk is
/// copied byte for byte. Bytes that aren't a well-formed PNG are returned
/// as is.
pub fn for_discord(png: &[u8]) -> Vec<u8> {
    const SIGNATURE: usize = 8;
    if png.len() < SIGNATURE {
        return png.to_vec();
    }
    let mut out = png[..SIGNATURE].to_vec();
    let mut rest = &png[SIGNATURE..];
    while rest.len() >= 12 {
        let len = u32::from_be_bytes(rest[..4].try_into().unwrap()) as usize;
        let Some(chunk) = rest.get(..len + 12) else {
            break;
        };
        if !matches!(&chunk[4..8], b"cICP" | b"mDCV") {
            out.extend_from_slice(chunk);
        }
        rest = &rest[len + 12..];
    }
    out.extend_from_slice(rest);
    out
}

/// The tone-mapped 8-bit sRGB pixels of the SDR output, RGB row by row.
/// `levels` are those of the full image, so that a preview made from a
/// downscaled copy is tone-mapped like the PNG.
pub fn sdr_rgb8(image: &ScRgbImage, levels: &LightLevels, options: &SdrOptions) -> Vec<u8> {
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
    let rgb = sdr_rgb8(image, levels, options);
    let (data, color) = match &image.alpha {
        None => (rgb, ColorType::Rgb),
        Some(alpha) => {
            let rgba = rgb
                .chunks_exact(3)
                .zip(alpha)
                .flat_map(|(p, &a)| [p[0], p[1], p[2], (a * 255.0).round() as u8])
                .collect();
            (rgba, ColorType::Rgba)
        }
    };
    let mut encoder = png::Encoder::new(out, image.width, image.height);
    encoder.set_color(color);
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
        ScRgbImage::new(width, height, pixels)
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
    fn max_cll_ignores_stray_pixels() {
        // Among 20,000 pixels at 100 nits, two 4000-nit specks are within
        // the brightest 0.01% and ignored; a third one is not.
        let mut levels = vec![100.0; 20_000];
        levels[7] = 4000.0;
        levels[8] = 4000.0;
        assert_eq!(max_content_light_level(&mut levels.clone()), 100.0);
        levels[9] = 4000.0;
        assert_eq!(max_content_light_level(&mut levels), 4000.0);
        assert_eq!(max_content_light_level(&mut []), 0.0);
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
        let cicp = info.coding_independent_code_points.unwrap();
        assert_eq!(
            (
                cicp.color_primaries,
                cicp.transfer_function,
                cicp.matrix_coefficients,
                cicp.is_video_full_range_image
            ),
            (9, 16, 0, true)
        );

        // The Discord copy is the same file minus cICP and mDCV, and still
        // decodes to the same pixels with the same profile.
        let stripped = for_discord(&bytes);
        assert_eq!(stripped.len(), bytes.len() - (12 + 4) - (12 + 24));
        let (stripped_info, stripped_data) = read_png(&stripped);
        assert!(stripped_info.coding_independent_code_points.is_none());
        assert!(stripped_info.mastering_display_color_volume.is_none());
        assert_eq!(stripped_info.content_light_level, info.content_light_level);
        assert_eq!(stripped_info.icc_profile, info.icc_profile);
        assert_eq!(stripped_data, data);
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
