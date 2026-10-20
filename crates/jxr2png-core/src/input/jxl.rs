//! JPEG XL input, through the pure-Rust `jxl-oxide` decoder.

use std::io::Cursor;

use jxl_oxide::{EnumColourEncoding, JxlImage, RenderingIntent};

use super::{InputOptions, to_scrgb};
use crate::color::{Primaries, SCRGB_WHITE_NITS, pq_decode};
use crate::decode::ScRgbImage;
use crate::{Error, Result};

/// Decodes the first frame of a JPEG XL file.
///
/// PQ and HLG files are rendered to BT.2100 PQ, which keeps their absolute
/// brightness (an HDR target means `jxl-oxide` doesn't tone-map), then read
/// back as nits. Float files without an HDR transfer function are
/// scene-linear, like OpenEXR. Everything else is SDR and, like an 8-bit
/// JPEG XR, gives only the SDR PNG.
pub(super) fn decode(bytes: &[u8], options: &InputOptions) -> Result<ScRgbImage> {
    let mut image = JxlImage::builder()
        .read(Cursor::new(bytes))
        .map_err(decode_error)?;
    // A pure-Rust color management system, for files described by an ICC
    // profile rather than by enumerated values.
    image.set_cms(jxl_oxide::Moxcms);
    let metadata = &image.image_header().metadata;
    if image.pixel_format().has_black() {
        return Err(Error::UnsupportedFormat("CMYK JPEG XL".into()));
    }
    let float = matches!(
        metadata.bit_depth,
        jxl_oxide::image::BitDepth::FloatSample { .. }
    );
    let intensity_target = metadata.tone_mapping.intensity_target;
    let hdr_signal = image.hdr_type().is_some();
    let target = if hdr_signal {
        EnumColourEncoding::bt2100_pq(RenderingIntent::Relative)
    } else {
        EnumColourEncoding::srgb_linear(RenderingIntent::Relative)
    };
    image.request_color_encoding(target);

    let render = image.render_frame(0).map_err(decode_error)?;
    let mut stream = render.stream();
    let (width, height, channels) = (stream.width(), stream.height(), stream.channels() as usize);
    let mut samples = vec![0f32; width as usize * height as usize * channels];
    stream.write_to_buffer(&mut samples);

    let color = |p: &[f32]| -> [f32; 3] {
        if channels < 3 {
            [p[0]; 3]
        } else {
            [p[0], p[1], p[2]]
        }
    };
    let (pixels, primaries, scale, hdr) = if hdr_signal {
        let pixels = samples
            .chunks_exact(channels)
            .map(|p| color(p).map(pq_decode))
            .collect();
        (pixels, Primaries::BT2020, 1.0 / SCRGB_WHITE_NITS, true)
    } else {
        let pixels = samples.chunks_exact(channels).map(color).collect();
        let scale = if float {
            // The default intensity target (255) only says "SDR", so it
            // doesn't count as the file's own white.
            options.scale(Some(intensity_target).filter(|&t| t != 255.0))
        } else {
            1.0
        };
        (pixels, Primaries::BT709, scale, float)
    };
    let alpha = (channels % 2 == 0).then(|| {
        samples
            .chunks_exact(channels)
            .map(|p| p[channels - 1].clamp(0.0, 1.0))
            .collect()
    });
    let mut image = to_scrgb(width, height, pixels, alpha, &primaries, scale);
    image.hdr = hdr;
    Ok(image)
}

/// Width and height from the image header.
pub(super) fn dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    let mut image = JxlImage::builder().build_uninit();
    image.feed_bytes(bytes).ok()?;
    match image.try_init().ok()? {
        jxl_oxide::InitializeResult::Initialized(image) => Some((image.width(), image.height())),
        jxl_oxide::InitializeResult::NeedMoreData(_) => None,
    }
}

fn decode_error(e: impl ToString) -> Error {
    Error::Decode(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PQ_GRADIENT: &[u8] = include_bytes!("../../tests/fixtures/pq_gradient.jxl");

    #[test]
    fn pq_file_keeps_absolute_brightness() {
        let image = decode(PQ_GRADIENT, &InputOptions::default()).unwrap();
        assert!(image.hdr);
        assert_eq!(dimensions(PQ_GRADIENT), Some((image.width, image.height)));
        let brightest = image
            .pixels
            .iter()
            .map(|p| p[0].max(p[1]).max(p[2]))
            .fold(0.0, f32::max);
        // A PQ gradient reaches far beyond SDR white.
        assert!(brightest * SCRGB_WHITE_NITS > 1000.0, "{brightest}");
    }

    #[test]
    fn sdr_file_is_marked_sdr() {
        let bytes = include_bytes!("../../tests/fixtures/sdr8.jxl");
        let image = decode(bytes, &InputOptions::default()).unwrap();
        assert!(!image.hdr);
        assert_eq!((image.width, image.height), (4, 2));
        // sRGB 200 decodes to about 0.58 linear.
        assert!(
            (image.pixels[0][0] - 0.578).abs() < 0.01,
            "{:?}",
            image.pixels[0]
        );
    }
}
