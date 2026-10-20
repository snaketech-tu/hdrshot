//! Radiance RGBE (`.hdr`) input: scene-linear RGB.

use image::ImageFormat;

use super::{InputOptions, to_scrgb};
use crate::color::Primaries;
use crate::decode::ScRgbImage;
use crate::{Error, Result};

/// Decodes a Radiance file. Its primaries are taken to be BT.709, which is
/// what nearly all writers use.
pub(super) fn decode(bytes: &[u8], options: &InputOptions) -> Result<ScRgbImage> {
    let image = image::load_from_memory_with_format(bytes, ImageFormat::Hdr)
        .map_err(|e| Error::Decode(e.to_string()))?
        .into_rgb32f();
    let (width, height) = image.dimensions();
    let pixels = image.into_raw().as_chunks::<3>().0.to_vec();
    Ok(to_scrgb(
        width,
        height,
        pixels,
        None,
        &Primaries::BT709,
        options.scale(None),
    ))
}

/// Width and height from the resolution line after the header, such as
/// `-Y 1080 +X 1920`.
pub(super) fn dimensions(head: &[u8]) -> Option<(u32, u32)> {
    let text = String::from_utf8_lossy(head);
    let line = text.split('\n').skip_while(|l| !l.is_empty()).nth(1)?;
    let parts: Vec<&str> = line.split_whitespace().collect();
    let [y_axis, rows, x_axis, cols] = parts[..] else {
        return None;
    };
    let (rows, cols) = (rows.parse().ok()?, cols.parse().ok()?);
    if y_axis.ends_with('Y') && x_axis.ends_with('X') {
        Some((cols, rows))
    } else {
        Some((rows, cols))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A flat (uncompressed) RGBE file.
    fn rgbe(pixels: &[[u8; 4]], width: usize) -> Vec<u8> {
        let height = pixels.len() / width;
        let mut bytes =
            format!("#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n\n-Y {height} +X {width}\n").into_bytes();
        for p in pixels {
            bytes.extend_from_slice(p);
        }
        bytes
    }

    #[test]
    fn decodes_rgbe() {
        // Mantissa 128 with exponent 129 is 1.0; exponent 130 doubles it.
        let bytes = rgbe(&[[128, 128, 128, 129], [128, 64, 0, 130]], 2);
        assert_eq!(dimensions(&bytes), Some((2, 1)));
        let image = decode(&bytes, &InputOptions::default()).unwrap();
        assert_eq!((image.width, image.height), (2, 1));
        for c in image.pixels[0] {
            assert!((c - 1.0).abs() < 0.01, "{:?}", image.pixels);
        }
        assert!((image.pixels[1][0] - 2.0).abs() < 0.02);
        assert!((image.pixels[1][1] - 1.0).abs() < 0.02);
    }
}
