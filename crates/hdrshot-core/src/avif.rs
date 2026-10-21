//! AVIF output: 10-bit AV1 in BT.2020 with the PQ transfer function.
//!
//! The same HDR picture as the HDR PNG, at a fraction of the size. Pixels
//! go through [`HdrTransform`] like the PNG's, so brightness, roll-off and
//! the `clli`/`mdcv` metadata match it. Encoding uses `rav1e` without its
//! assembly, so it builds with Rust alone.

use std::io::Write;

use avif_serialize::Aviffy;
use avif_serialize::constants::TransferCharacteristics as AvifTransfer;
use avif_serialize::constants::{
    ColorPrimaries as AvifPrimaries, MatrixCoefficients as AvifMatrix,
};
use rav1e::prelude::*;
use rayon::prelude::*;

use crate::color::pq_encode;
use crate::decode::ScRgbImage;
use crate::encode::{
    BT2020_PRIMARIES_XY, HdrTransform, MASTERING_MIN_NITS, to_clli_units, xy_units,
};
use crate::{Error, HdrOptions, Result};

/// Settings for the AVIF output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AvifOptions {
    /// 1 to 100; higher is closer to the original and larger.
    pub quality: u8,
    /// Encoder speed from 0 (slowest, smallest) to 10 (fastest).
    pub speed: u8,
    /// Keep full-resolution color (4:4:4). Off halves color resolution
    /// (4:2:0), which makes the file smaller but softens colored text and
    /// thin colored edges.
    pub full_color: bool,
}

impl Default for AvifOptions {
    fn default() -> Self {
        Self {
            quality: 90,
            // Speeds below 8 take much longer for no smaller file on
            // screenshots; 10 is twice as fast again but 15% larger.
            speed: 8,
            full_color: true,
        }
    }
}

/// BT.2020 non-constant-luminance weights of red and blue.
const KR: f32 = 0.2627;
const KB: f32 = 0.0593;

/// Largest 10-bit code value.
const MAX_CODE: f32 = 1023.0;

/// Writes `image` as an HDR AVIF.
pub fn write_hdr_avif<W: Write>(
    image: &ScRgbImage,
    hdr: &HdrOptions,
    avif: &AvifOptions,
    mut out: W,
) -> Result<()> {
    let (width, height) = (image.width as usize, image.height as usize);
    let transform = HdrTransform::new(image, hdr);

    // Full-resolution Y, Cb and Cr in 0..1, from the PQ-encoded BT.2020 RGB.
    let mut ycbcr = vec![[0f32; 3]; width * height];
    let brightest: Vec<f32> = ycbcr
        .par_iter_mut()
        .zip(image.pixels.par_iter())
        .map(|(out, &p)| {
            let nits = transform.nits(p);
            let [r, g, b] = nits.map(pq_encode);
            let y = KR * r + (1.0 - KR - KB) * g + KB * b;
            *out = [
                y,
                (b - y) / (2.0 * (1.0 - KB)) + 0.5,
                (r - y) / (2.0 * (1.0 - KR)) + 0.5,
            ];
            nits.into_iter().fold(0.0, f32::max)
        })
        .collect();
    let stats = transform.stats(brightest);

    let chroma = if avif.full_color {
        ChromaSampling::Cs444
    } else {
        ChromaSampling::Cs420
    };
    let color = encode_av1(width, height, chroma, avif, true, |frame| {
        fill_color(frame, &ycbcr, width, height, avif.full_color);
    })?;
    drop(ycbcr);
    let alpha = match &image.alpha {
        Some(alpha) => Some(encode_av1(
            width,
            height,
            ChromaSampling::Cs400,
            avif,
            false,
            |frame| {
                let mut plane = frame.planes[0].mut_slice(Default::default());
                for (row, values) in plane.rows_iter_mut().zip(alpha.chunks_exact(width)) {
                    for (o, &a) in row.iter_mut().zip(values) {
                        *o = to_code(a);
                    }
                }
            },
        )?),
        None => None,
    };

    let mut container = Aviffy::new();
    container
        .set_color_primaries(AvifPrimaries::Bt2020)
        .set_transfer_characteristics(AvifTransfer::Smpte2084)
        .set_matrix_coefficients(AvifMatrix::Bt2020Ncl)
        .set_full_color_range(true)
        .set_chroma_subsampling((!avif.full_color, !avif.full_color))
        .set_content_light_level(
            stats.max_cll.round().clamp(0.0, 10_000.0) as u16,
            stats.max_fall.round().clamp(0.0, 10_000.0) as u16,
        );
    let [rx, ry, gx, gy, bx, by, wx, wy] = BT2020_PRIMARIES_XY.map(xy_units);
    container.set_mastering_display(
        [(gx, gy), (bx, by), (rx, ry)],
        (wx, wy),
        to_clli_units(stats.mastering_peak),
        to_clli_units(MASTERING_MIN_NITS),
    );
    container
        .write(
            &mut out,
            &color,
            alpha.as_deref(),
            image.width,
            image.height,
            10,
        )
        .map_err(Error::Io)?;
    Ok(())
}

fn to_code(v: f32) -> u16 {
    (v.clamp(0.0, 1.0) * MAX_CODE).round() as u16
}

/// Copies Y, Cb and Cr into the frame, averaging each 2×2 block of chroma
/// for 4:2:0.
fn fill_color(
    frame: &mut Frame<u16>,
    ycbcr: &[[f32; 3]],
    width: usize,
    height: usize,
    full_color: bool,
) {
    let [y_plane, cb_plane, cr_plane] = &mut frame.planes;
    let mut y_rows = y_plane.mut_slice(Default::default());
    for (row, pixels) in y_rows.rows_iter_mut().zip(ycbcr.chunks_exact(width)) {
        for (o, p) in row.iter_mut().zip(pixels) {
            *o = to_code(p[0]);
        }
    }
    for (plane, c) in [(cb_plane, 1), (cr_plane, 2)] {
        let mut rows = plane.mut_slice(Default::default());
        if full_color {
            for (row, pixels) in rows.rows_iter_mut().zip(ycbcr.chunks_exact(width)) {
                for (o, p) in row.iter_mut().zip(pixels) {
                    *o = to_code(p[c]);
                }
            }
        } else {
            for (cy, row) in rows.rows_iter_mut().take(height.div_ceil(2)).enumerate() {
                for (cx, o) in row.iter_mut().take(width.div_ceil(2)).enumerate() {
                    let (mut sum, mut n) = (0.0, 0.0);
                    for y in 2 * cy..(2 * cy + 2).min(height) {
                        for x in 2 * cx..(2 * cx + 2).min(width) {
                            sum += ycbcr[y * width + x][c];
                            n += 1.0;
                        }
                    }
                    *o = to_code(sum / n);
                }
            }
        }
    }
}

/// Maps quality 1..100 to an AV1 quantizer 0..255, the same curve `ravif`
/// uses, so qualities match what other AVIF tools mean by them.
fn quantizer(quality: u8) -> usize {
    let q = f32::from(quality.clamp(1, 100)) / 100.0;
    let x = if q >= 0.82 {
        (1.0 - q) * 2.6
    } else if q > 0.25 {
        q.mul_add(-0.5, 1.0 - 0.125)
    } else {
        1.0 - q
    };
    (x * 255.0).round() as usize
}

/// Encodes one 10-bit still frame, filled by `fill`.
fn encode_av1(
    width: usize,
    height: usize,
    chroma: ChromaSampling,
    options: &AvifOptions,
    color: bool,
    fill: impl FnOnce(&mut Frame<u16>),
) -> Result<Vec<u8>> {
    let threads = rayon::current_num_threads();
    let quantizer = quantizer(options.quality);
    let mut speed_settings = SpeedSettings::from_preset(options.speed.min(10));
    speed_settings.scene_detection_mode = SceneDetectionSpeed::None;
    speed_settings.rdo_lookahead_frames = 1;
    let config = EncoderConfig {
        width,
        height,
        bit_depth: 10,
        chroma_sampling: chroma,
        pixel_range: PixelRange::Full,
        color_description: color.then_some(ColorDescription {
            color_primaries: ColorPrimaries::BT2020,
            transfer_characteristics: TransferCharacteristics::SMPTE2084,
            matrix_coefficients: MatrixCoefficients::BT2020NCL,
        }),
        still_picture: true,
        quantizer,
        min_quantizer: quantizer as u8,
        tiles: threads.min(width * height / (128 * 128)).max(1),
        speed_settings,
        ..Default::default()
    };
    let mut context: Context<u16> = Config::new()
        .with_encoder_config(config)
        .with_threads(threads)
        .new_context()
        .map_err(|e| encode_error(e.to_string()))?;
    let mut frame = context.new_frame();
    fill(&mut frame);
    context
        .send_frame(frame)
        .map_err(|e| encode_error(e.to_string()))?;
    context.flush();
    let mut data = Vec::new();
    loop {
        match context.receive_packet() {
            Ok(mut packet) => data.append(&mut packet.data),
            Err(EncoderStatus::Encoded) => {}
            Err(EncoderStatus::LimitReached) => break,
            Err(e) => return Err(encode_error(e.to_string())),
        }
    }
    Ok(data)
}

fn encode_error(message: String) -> Error {
    Error::EncodeAvif(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find_box<'a>(file: &'a [u8], name: &[u8; 4]) -> Option<&'a [u8]> {
        let at = file.windows(4).position(|w| w == name)?;
        let size = u32::from_be_bytes(file[at - 4..at].try_into().unwrap()) as usize;
        file.get(at + 4..at - 4 + size)
    }

    #[test]
    fn avif_is_tagged_as_bt2020_pq() {
        // A gradient up to 1000 nits with a transparent corner.
        let pixels = (0..64 * 48).map(|i| [(i % 64) as f32 / 5.0; 3]).collect();
        let mut image = ScRgbImage::new(64, 48, pixels);
        let mut alpha = vec![1.0; 64 * 48];
        alpha[0] = 0.0;
        image.alpha = Some(alpha);
        for full_color in [true, false] {
            let mut file = Vec::new();
            let options = AvifOptions {
                full_color,
                speed: 10,
                ..Default::default()
            };
            write_hdr_avif(&image, &HdrOptions::default(), &options, &mut file).unwrap();
            assert_eq!(&file[4..12], b"ftypavif");
            // nclx: primaries 9, transfer 16, matrix 9, full range.
            let colr = find_box(&file, b"colr").unwrap();
            assert_eq!(&colr[..4], b"nclx");
            assert_eq!(&colr[4..11], &[0, 9, 0, 16, 0, 9, 0x80]);
            let clli = find_box(&file, b"clli").unwrap();
            let max_cll = u16::from_be_bytes([clli[0], clli[1]]);
            assert!((max_cll as i32 - 1008).abs() <= 10, "{max_cll}");
            assert!(find_box(&file, b"mdcv").is_some());
            // The alpha plane is its own item.
            assert!(
                file.windows(28)
                    .any(|w| w == b"urn:mpeg:mpegB:cicp:systems:")
            );
        }
    }

    #[test]
    fn quality_maps_like_ravif() {
        assert_eq!(quantizer(100), 0);
        assert!(quantizer(90) < quantizer(80));
        assert_eq!(quantizer(1), 252);
    }
}
