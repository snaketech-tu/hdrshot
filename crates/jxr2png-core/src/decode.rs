//! JPEG XR decoding to linear scRGB.

use jxr::{
    AlphaMode, ChannelLayout, DecodeRequest, DecodedSamples, JxrView, PixelFormat, SampleFormat,
};

use crate::color::srgb_decode;
use crate::{Error, Result};

/// A decoded image in linear scRGB (BT.709 primaries, 1.0 = 80 nits).
#[derive(Debug, Clone)]
pub struct ScRgbImage {
    pub width: u32,
    pub height: u32,
    /// Row-major RGB pixels.
    pub pixels: Vec<[f32; 3]>,
}

/// Decode a `.jxr` file's bytes.
///
/// Float, half-float and fixed-point sources are linear scRGB already, which
/// is what Windows HDR screenshots use. 8-bit sources are treated as sRGB and
/// linearized.
pub fn decode(bytes: &[u8]) -> Result<ScRgbImage> {
    let view = JxrView::parse(bytes).map_err(decode_error)?;
    let info = view.info();
    let (width, height) = (info.width, info.height);
    let sample_format = info.primary.sample_format;
    // The decoder only offers layouts matching the source, so alpha is
    // decoded when present and dropped below.
    let (layout, channels) = if matches!(info.alpha_mode, AlphaMode::None) {
        (ChannelLayout::Rgb, 3)
    } else {
        (ChannelLayout::Rgba, 4)
    };

    let pixels = match sample_format {
        SampleFormat::Float16 | SampleFormat::Float32 | SampleFormat::FixedPoint { .. } => {
            let request = DecodeRequest::new(PixelFormat::F32(layout));
            let image = view.decoder().decode(&request).map_err(decode_error)?;
            match image.samples {
                DecodedSamples::F32(samples) => to_pixels(&samples, channels, |v| v),
                _ => return Err(unsupported(sample_format)),
            }
        }
        SampleFormat::Unsigned { bits: 8 } => {
            let request = DecodeRequest::new(PixelFormat::U8(layout));
            let image = view.decoder().decode(&request).map_err(decode_error)?;
            match image.samples {
                DecodedSamples::U8(samples) => {
                    to_pixels(&samples, channels, |v| srgb_decode(f32::from(v) / 255.0))
                }
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
    })
}

fn to_pixels<T: Copy>(samples: &[T], channels: usize, f: impl Fn(T) -> f32) -> Vec<[f32; 3]> {
    samples
        .chunks_exact(channels)
        .map(|p| [f(p[0]), f(p[1]), f(p[2])])
        .collect()
}

fn decode_error(e: jxr::JxrError) -> Error {
    Error::Decode(e.to_string())
}

fn unsupported(format: SampleFormat) -> Error {
    Error::UnsupportedFormat(format!("{format:?}"))
}
