//! JPEG XR decoding to linear scRGB.

use jxr::{
    AlphaMode, ChannelLayout, DecodeRequest, DecodedSamples, JxrView, PixelFormat, SampleFormat,
};

use std::fs::File;
use std::io::Read;
use std::path::Path;

use crate::color::srgb_decode;
use crate::{Error, Result};

/// A decoded image in linear scRGB (BT.709 primaries, 1.0 = 80 nits).
#[derive(Debug, Clone)]
pub struct ScRgbImage {
    pub width: u32,
    pub height: u32,
    /// Row-major RGB pixels.
    pub pixels: Vec<[f32; 3]>,
    /// Straight (not premultiplied) alpha from 0 to 1 for each pixel, if
    /// the source has an alpha channel that isn't fully opaque.
    pub alpha: Option<Vec<f32>>,
    /// Whether the source is HDR (float or fixed point) rather than 8-bit
    /// SDR.
    pub hdr: bool,
    /// The brightness, in nits, of the source's own SDR white, for sources
    /// that have one: gain map photos, which are an SDR picture made
    /// brighter. SDR outputs use it unless asked for another white (see
    /// [`crate::SdrOptions::for_image`]).
    pub sdr_white: Option<f32>,
}

impl ScRgbImage {
    /// An opaque HDR image.
    pub fn new(width: u32, height: u32, pixels: Vec<[f32; 3]>) -> Self {
        Self {
            width,
            height,
            pixels,
            alpha: None,
            hdr: true,
            sdr_white: None,
        }
    }
}

/// Decode a `.jxr` file's bytes.
///
/// Float, half-float and fixed-point sources are linear scRGB already, which
/// is what Windows HDR screenshots use. 8-bit sources are treated as sRGB and
/// linearized, and marked as SDR. Alpha is kept only if some pixel isn't
/// fully opaque: Game Bar captures carry an alpha plane that is always 1.
pub fn decode(bytes: &[u8]) -> Result<ScRgbImage> {
    let view = JxrView::parse(bytes).map_err(decode_error)?;
    let info = view.info();
    let (width, height) = (info.width, info.height);
    let sample_format = info.primary.sample_format;
    // The decoder only offers layouts matching the source.
    let (layout, channels) = if matches!(info.alpha_mode, AlphaMode::None) {
        (ChannelLayout::Rgb, 3)
    } else {
        (ChannelLayout::Rgba, 4)
    };

    let (pixels, alpha, hdr) = match sample_format {
        SampleFormat::Float16 | SampleFormat::Float32 | SampleFormat::FixedPoint { .. } => {
            let request = DecodeRequest::new(PixelFormat::F32(layout));
            let image = view.decoder().decode(&request).map_err(decode_error)?;
            match image.samples {
                DecodedSamples::F32(samples) => (
                    to_pixels(&samples, channels, |v| v),
                    to_alpha(&samples, channels, |v| v),
                    true,
                ),
                _ => return Err(unsupported(sample_format)),
            }
        }
        SampleFormat::Unsigned { bits: 8 } => {
            let request = DecodeRequest::new(PixelFormat::U8(layout));
            let image = view.decoder().decode(&request).map_err(decode_error)?;
            match image.samples {
                DecodedSamples::U8(samples) => (
                    to_pixels(&samples, channels, |v| srgb_decode(f32::from(v) / 255.0)),
                    to_alpha(&samples, channels, |v| f32::from(v) / 255.0),
                    false,
                ),
                _ => return Err(unsupported(sample_format)),
            }
        }
        other => return Err(unsupported(other)),
    };

    if pixels.len() != width as usize * height as usize {
        return Err(Error::Decode(format!(
            "decoder returned {} pixels for a {width}x{height} image",
            pixels.len()
        )));
    }
    Ok(ScRgbImage {
        width,
        height,
        pixels,
        alpha,
        hdr,
        sdr_white: None,
    })
}

/// Read a `.jxr` file's width and height without decoding it.
///
/// Reads only the start of the file, where Windows and the JPEG XR
/// reference encoder put the tag directory. Files that put it elsewhere are
/// read in full.
pub fn dimensions(path: &Path) -> Result<(u32, u32)> {
    let mut head = Vec::new();
    File::open(path)?.take(64 * 1024).read_to_end(&mut head)?;
    if let Some(size) = tag_dimensions(&head) {
        return Ok(size);
    }
    let bytes = std::fs::read(path)?;
    let info = JxrView::parse(&bytes).map_err(decode_error)?.info().clone();
    Ok((info.width, info.height))
}

/// Width and height from the container's tag directory (ImageWidth 0xBC80,
/// ImageHeight 0xBC81), if it lies within `head`.
fn tag_dimensions(head: &[u8]) -> Option<(u32, u32)> {
    let u16_at = |at: usize| Some(u16::from_le_bytes(head.get(at..at + 2)?.try_into().ok()?));
    let u32_at = |at: usize| Some(u32::from_le_bytes(head.get(at..at + 4)?.try_into().ok()?));
    if head.get(..3)? != b"II\xbc" {
        return None;
    }
    let dir = usize::try_from(u32_at(4)?).ok()?;
    let (mut width, mut height) = (None, None);
    for i in 0..usize::from(u16_at(dir)?) {
        let entry = dir + 2 + 12 * i;
        let value = match u16_at(entry + 2)? {
            3 => u32::from(u16_at(entry + 8)?),
            4 => u32_at(entry + 8)?,
            _ => continue,
        };
        match u16_at(entry)? {
            0xBC80 => width = Some(value),
            0xBC81 => height = Some(value),
            _ => {}
        }
    }
    Some((width.filter(|&w| w > 0)?, height.filter(|&h| h > 0)?))
}

fn to_pixels<T: Copy>(samples: &[T], channels: usize, f: impl Fn(T) -> f32) -> Vec<[f32; 3]> {
    samples
        .chunks_exact(channels)
        .map(|p| [f(p[0]), f(p[1]), f(p[2])])
        .collect()
}

/// The alpha channel, the last of `channels`, unless there is none or every
/// pixel is opaque.
fn to_alpha<T: Copy>(samples: &[T], channels: usize, f: impl Fn(T) -> f32) -> Option<Vec<f32>> {
    if channels < 4 {
        return None;
    }
    let alpha: Vec<f32> = samples
        .chunks_exact(channels)
        .map(|p| f(p[channels - 1]).clamp(0.0, 1.0))
        .collect();
    alpha.iter().any(|&a| a < 1.0).then_some(alpha)
}

fn decode_error(e: jxr::JxrError) -> Error {
    Error::Decode(e.to_string())
}

fn unsupported(format: SampleFormat) -> Error {
    Error::UnsupportedFormat(format!("{format:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A container header with one SHORT and one LONG size tag.
    fn header(width: u16, height: u32) -> Vec<u8> {
        let mut h = b"II\xbc\x01".to_vec();
        h.extend(8u32.to_le_bytes());
        h.extend(3u16.to_le_bytes());
        // An unrelated tag, then ImageWidth (SHORT) and ImageHeight (LONG).
        for (tag, kind, value) in [
            (0xBC01u16, 1u16, 0u32),
            (0xBC80, 3, u32::from(width)),
            (0xBC81, 4, height),
        ] {
            h.extend(tag.to_le_bytes());
            h.extend(kind.to_le_bytes());
            h.extend(1u32.to_le_bytes());
            h.extend(value.to_le_bytes());
        }
        h
    }

    #[test]
    fn reads_size_tags() {
        assert_eq!(tag_dimensions(&header(3440, 1440)), Some((3440, 1440)));
    }

    #[test]
    fn rejects_other_or_cut_off_data() {
        assert_eq!(tag_dimensions(b"\x89PNG\r\n\x1a\n"), None);
        let h = header(3440, 1440);
        assert_eq!(tag_dimensions(&h[..h.len() - 4]), None);
    }
}
