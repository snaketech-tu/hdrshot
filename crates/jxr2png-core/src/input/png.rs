//! HDR PNG input: PQ or HLG, marked by a `cICP` chunk or by the ICC
//! profile's `cicp` tag.

use std::io::Cursor;

use ::png::{BitDepth, ColorType, Decoder, Transformations};
use rayon::prelude::*;

use super::to_scrgb;
use crate::color::{Primaries, SCRGB_WHITE_NITS, hlg_decode, pq_decode};
use crate::decode::ScRgbImage;
use crate::{Error, Result};

/// Nominal peak of an HLG display, in nits (BT.2100).
const HLG_PEAK_NITS: f32 = 1000.0;

/// How the samples of an HDR PNG are encoded.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Signal {
    primaries: Primaries,
    hlg: bool,
    full_range: bool,
}

impl Signal {
    /// The signal described by coding-independent code points (ITU-T
    /// H.273), if it's one that can be read: PQ or HLG, in BT.709, BT.2020
    /// or Display P3, as RGB.
    fn from_cicp(primaries: u8, transfer: u8, matrix: u8, full_range: bool) -> Result<Self> {
        let primaries = match primaries {
            1 => Primaries::BT709,
            9 => Primaries::BT2020,
            12 => Primaries::DISPLAY_P3,
            other => {
                return Err(Error::UnsupportedFormat(format!(
                    "PNG color primaries {other}"
                )));
            }
        };
        let hlg = match transfer {
            16 => false,
            18 => true,
            _ => return Err(not_hdr()),
        };
        if matrix != 0 {
            return Err(Error::UnsupportedFormat(format!(
                "PNG matrix coefficients {matrix}"
            )));
        }
        Ok(Self {
            primaries,
            hlg,
            full_range,
        })
    }
}

fn not_hdr() -> Error {
    Error::NotHdr("the PNG is not marked as PQ or HLG HDR".into())
}

pub(super) fn decode(bytes: &[u8]) -> Result<ScRgbImage> {
    let mut decoder = Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(Transformations::EXPAND);
    let mut reader = decoder.read_info().map_err(decode_error)?;
    let info = reader.info();
    let signal = match (&info.coding_independent_code_points, &info.icc_profile) {
        (Some(c), _) => Signal::from_cicp(
            c.color_primaries,
            c.transfer_function,
            c.matrix_coefficients,
            c.is_video_full_range_image,
        )?,
        (None, Some(icc)) => {
            let [p, t, m, f] = icc_cicp(icc).ok_or_else(not_hdr)?;
            Signal::from_cicp(p, t, m, f != 0)?
        }
        (None, None) => return Err(not_hdr()),
    };
    let (width, height) = (info.width, info.height);

    let mut buf = vec![
        0;
        reader
            .output_buffer_size()
            .ok_or_else(|| decode_error("image too large"))?
    ];
    let frame = reader.next_frame(&mut buf).map_err(decode_error)?;
    buf.truncate(frame.buffer_size());
    let channels = match frame.color_type {
        ColorType::Grayscale => 1,
        ColorType::GrayscaleAlpha => 2,
        ColorType::Rgb => 3,
        ColorType::Rgba => 4,
        ColorType::Indexed => return Err(decode_error("palette was not expanded")),
    };
    let (samples, max): (Vec<f32>, f32) = match frame.bit_depth {
        BitDepth::Sixteen => (
            buf.as_chunks::<2>()
                .0
                .iter()
                .map(|&b| f32::from(u16::from_be_bytes(b)))
                .collect(),
            65535.0,
        ),
        _ => (buf.iter().map(|&b| f32::from(b)).collect(), 255.0),
    };
    // Narrow ("video") range puts black and white at 16 and 235 on an
    // 8-bit scale.
    let (black, white) = if signal.full_range {
        (0.0, max)
    } else {
        let step = (max + 1.0) / 256.0;
        (16.0 * step, 235.0 * step)
    };
    let normalize = |v: f32| ((v - black) / (white - black)).clamp(0.0, 1.0);

    let luma = luminance_weights(&signal.primaries);
    let pixels: Vec<[f32; 3]> = samples
        .par_chunks_exact(channels)
        .map(|p| {
            let rgb = if channels < 3 {
                [normalize(p[0]); 3]
            } else {
                [normalize(p[0]), normalize(p[1]), normalize(p[2])]
            };
            if signal.hlg {
                let scene = rgb.map(hlg_decode);
                let y = scene[0] * luma[0] + scene[1] * luma[1] + scene[2] * luma[2];
                // BT.2100 HLG OOTF with system gamma 1.2 for a 1000-nit display.
                let gain = HLG_PEAK_NITS * y.max(0.0).powf(0.2);
                scene.map(|c| c * gain)
            } else {
                rgb.map(pq_decode)
            }
        })
        .collect();
    let alpha = (channels % 2 == 0).then(|| {
        samples
            .chunks_exact(channels)
            .map(|p| p[channels - 1] / max)
            .collect()
    });
    Ok(to_scrgb(
        width,
        height,
        pixels,
        alpha,
        &signal.primaries,
        1.0 / SCRGB_WHITE_NITS,
    ))
}

/// The four code points of an ICC profile's `cicp` tag, if it has one.
fn icc_cicp(icc: &[u8]) -> Option<[u8; 4]> {
    let u32_at = |at: usize| Some(u32::from_be_bytes(icc.get(at..at + 4)?.try_into().ok()?));
    let count = u32_at(128)? as usize;
    (0..count.min(100)).find_map(|i| {
        let entry = 132 + 12 * i;
        if icc.get(entry..entry + 4)? != b"cicp" {
            return None;
        }
        let offset = u32_at(entry + 4)? as usize;
        let data = icc.get(offset..offset + 12)?;
        (&data[..4] == b"cicp").then(|| data[8..12].try_into().unwrap())
    })
}

/// The Y row of the RGB to XYZ matrix of `primaries`.
fn luminance_weights(primaries: &Primaries) -> [f32; 3] {
    if *primaries == Primaries::BT709 {
        [0.2126, 0.7152, 0.0722]
    } else if *primaries == Primaries::DISPLAY_P3 {
        [0.2290, 0.6917, 0.0793]
    } else {
        [0.2627, 0.6780, 0.0593]
    }
}

/// Width and height from the `IHDR` chunk.
pub(super) fn dimensions(head: &[u8]) -> Option<(u32, u32)> {
    let u32_at = |at: usize| Some(u32::from_be_bytes(head.get(at..at + 4)?.try_into().ok()?));
    (head.get(12..16)? == b"IHDR").then_some((u32_at(16)?, u32_at(20)?))
}

fn decode_error(e: impl ToString) -> Error {
    Error::Decode(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HdrOptions, SdrOptions, light_levels, write_hdr_png, write_sdr_png};

    #[test]
    fn reads_back_our_hdr_png() {
        // Black, scRGB white (80 nits), 1000 nits and a saturated red.
        let source = ScRgbImage::new(4, 1, vec![[0.0; 3], [1.0; 3], [12.5; 3], [3.0, 0.2, 0.1]]);
        let mut bytes = Vec::new();
        let levels = light_levels(&source);
        write_hdr_png(
            &source,
            &levels,
            &HdrOptions::default(),
            &SdrOptions::default(),
            &mut bytes,
        )
        .unwrap();
        assert_eq!(dimensions(&bytes), Some((4, 1)));
        let check = |bytes: &[u8]| {
            let image = decode(bytes).unwrap();
            assert!(image.hdr && image.alpha.is_none());
            for (got, want) in image.pixels.iter().zip(&source.pixels) {
                for (g, w) in got.iter().zip(want) {
                    // Ten-bit PQ steps are under 1% apart in this range.
                    assert!(
                        (g - w).abs() <= w.abs() * 0.02 + 0.002,
                        "{got:?} vs {want:?}"
                    );
                }
            }
        };
        check(&bytes);
        // Without the cICP chunk, the ICC profile's cicp tag marks it.
        check(&crate::for_discord(&bytes));
    }

    #[test]
    fn rejects_sdr_png() {
        let source = ScRgbImage::new(1, 1, vec![[0.5; 3]]);
        let mut bytes = Vec::new();
        write_sdr_png(
            &source,
            &light_levels(&source),
            &SdrOptions::default(),
            &mut bytes,
        )
        .unwrap();
        assert!(matches!(decode(&bytes), Err(Error::NotHdr(_))));
    }

    #[test]
    fn hlg_reference_white_is_about_203_nits() {
        // BT.2408: HLG 75% is reference white, 203 nits on a 1000-nit display.
        let signal = Signal::from_cicp(9, 18, 0, true).unwrap();
        assert!(signal.hlg);
        let scene = hlg_decode(0.75);
        let nits = HLG_PEAK_NITS * scene.powf(0.2) * scene;
        assert!((nits - 203.0).abs() < 3.0, "{nits}");
    }
}
