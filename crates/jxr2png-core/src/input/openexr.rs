//! OpenEXR input: scene-linear half or float RGB(A).

use ::exr::prelude::*;

use super::{InputOptions, to_scrgb};
use crate::color::Primaries;
use crate::decode::ScRgbImage;
use crate::{Error, Result as CoreResult};

struct Pixels {
    width: usize,
    rgb: Vec<[f32; 3]>,
    alpha: Option<Vec<f32>>,
}

/// Decodes the first layer with RGB channels. Colors use the file's
/// `chromaticities` attribute, or BT.709 if it has none, as the OpenEXR
/// spec says. Alpha in OpenEXR is premultiplied and is undone here.
pub(super) fn decode(bytes: &[u8], options: &InputOptions) -> CoreResult<ScRgbImage> {
    let image = read()
        .no_deep_data()
        .largest_resolution_level()
        .rgba_channels(
            |size, channels: &RgbaChannels| Pixels {
                width: size.width(),
                rgb: vec![[0.0; 3]; size.area()],
                alpha: channels.3.as_ref().map(|_| vec![1.0; size.area()]),
            },
            |pixels: &mut Pixels, at, (r, g, b, a): (f32, f32, f32, f32)| {
                let i = at.y() * pixels.width + at.x();
                pixels.rgb[i] = [r, g, b];
                if let Some(alpha) = &mut pixels.alpha {
                    alpha[i] = a;
                }
            },
        )
        .first_valid_layer()
        .all_attributes()
        .from_buffered(std::io::Cursor::new(bytes))
        .map_err(|e| Error::Decode(e.to_string()))?;

    let layer = image.layer_data;
    let size = layer.size;
    let Pixels {
        mut rgb, mut alpha, ..
    } = layer.channel_data.pixels;
    if let Some(alpha) = &mut alpha {
        for (p, a) in rgb.iter_mut().zip(alpha.iter_mut()) {
            *a = a.clamp(0.0, 1.0);
            if *a > 0.0 {
                *p = p.map(|c| c / *a);
            }
        }
    }
    let primaries = image
        .attributes
        .chromaticities
        .map_or(Primaries::BT709, |c| {
            let xy = |v: Vec2<f32>| [f64::from(v.x()), f64::from(v.y())];
            Primaries {
                red: xy(c.red),
                green: xy(c.green),
                blue: xy(c.blue),
                white: xy(c.white),
            }
        });
    let scale = options.scale(layer.attributes.white_luminance);
    Ok(to_scrgb(
        u32::try_from(size.width()).map_err(|_| Error::Decode("image too wide".into()))?,
        u32::try_from(size.height()).map_err(|_| Error::Decode("image too tall".into()))?,
        rgb,
        alpha,
        &primaries,
        scale,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_exr(pixels: &[(f32, f32, f32, f32)], width: usize, white: Option<f32>) -> Vec<u8> {
        let height = pixels.len() / width;
        let channels = SpecificChannels::rgba(|at: Vec2<usize>| pixels[at.y() * width + at.x()]);
        let mut layer = Layer::new(
            (width, height),
            LayerAttributes::default(),
            Encoding::FAST_LOSSLESS,
            channels,
        );
        layer.attributes.white_luminance = white;
        let mut out = std::io::Cursor::new(Vec::new());
        Image::from_layer(layer)
            .write()
            .to_buffered(&mut out)
            .unwrap();
        out.into_inner()
    }

    #[test]
    fn reads_linear_rgb_and_unpremultiplies_alpha() {
        let bytes = write_exr(&[(1.0, 0.5, 0.25, 1.0), (2.0, 2.0, 2.0, 0.5)], 2, None);
        let image = decode(&bytes, &InputOptions::default()).unwrap();
        assert_eq!((image.width, image.height), (2, 1));
        assert!(image.hdr);
        for (a, b) in image.pixels[0].iter().zip([1.0, 0.5, 0.25]) {
            assert!((a - b).abs() < 1e-3);
        }
        for c in image.pixels[1] {
            assert!((c - 4.0).abs() < 1e-3);
        }
        assert_eq!(image.alpha.as_deref(), Some(&[1.0, 0.5][..]));
    }

    #[test]
    fn white_luminance_sets_brightness() {
        let bytes = write_exr(&[(1.0, 1.0, 1.0, 1.0)], 1, Some(160.0));
        let image = decode(&bytes, &InputOptions::default()).unwrap();
        assert!((image.pixels[0][0] - 2.0).abs() < 1e-3);
        assert!(image.alpha.is_none());
    }
}
