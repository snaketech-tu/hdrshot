//! hdrshot-core for the browser version of hdrshot (`web/` in the
//! repository), built with wasm-bindgen.
//!
//! Each image is converted in a Web Worker: the page hands over the file's
//! bytes and gets the files back, so nothing leaves the visitor's computer.
//! The conversion is [`hdrshot_core::convert_image`], the same code the
//! desktop app and CLI use.

use hdrshot_core::{
    AvifOptions, ConvertOptions, HdrFormat, HdrOptions, InputFormat, InputOptions, OutputMode,
    ScRgbImage, SdrFormat, SdrOptions, convert_image, input, preview_image, sdr_rgb8,
};
use wasm_bindgen::prelude::*;

/// The version of hdrshot this build is from.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").into()
}

/// The conversion settings, as on the page. Zero means "not set" for the
/// numbers that are optional in [`ConvertOptions`].
#[wasm_bindgen]
#[derive(Debug, Clone, Copy)]
pub struct Settings {
    pub hdr: bool,
    pub sdr: bool,
    /// 0 HDR PNG, 1 AVIF, 2 Ultra HDR JPEG.
    pub hdr_format: u8,
    pub avif_quality: u8,
    pub avif_speed: u8,
    pub avif_full_color: bool,
    pub for_discord: bool,
    /// Brightest highlight of the HDR image in nits; 0 keeps the original.
    pub hdr_peak: f32,
    /// Write the SDR image as JPEG instead of PNG.
    pub sdr_jpeg: bool,
    pub jpeg_quality: u8,
    pub sdr_white: f32,
    /// HDR brightness that maps to the SDR maximum; 0 picks it per image.
    pub sdr_peak: f32,
    /// Brightness of 1.0 in scene-referred files; 0 uses the file's own.
    pub input_white: f32,
    /// Scale taller images down to this height; 0 keeps the source size.
    pub max_height: u32,
}

#[wasm_bindgen]
impl Settings {
    /// The defaults of the desktop app and CLI.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        let defaults = ConvertOptions::default();
        Self {
            hdr: defaults.mode.hdr(),
            sdr: defaults.mode.sdr(),
            hdr_format: 0,
            avif_quality: defaults.hdr.avif.quality,
            avif_speed: defaults.hdr.avif.speed,
            avif_full_color: defaults.hdr.avif.full_color,
            for_discord: defaults.hdr.for_discord,
            hdr_peak: 0.0,
            sdr_jpeg: false,
            jpeg_quality: SdrFormat::DEFAULT_JPEG_QUALITY,
            sdr_white: defaults.sdr.white_nits,
            sdr_peak: 0.0,
            input_white: 0.0,
            max_height: 0,
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self::new()
    }
}

impl Settings {
    fn options(&self) -> ConvertOptions {
        let set = |v: f32| (v > 0.0).then_some(v);
        ConvertOptions {
            mode: match (self.hdr, self.sdr) {
                (true, false) => OutputMode::Hdr,
                (false, true) => OutputMode::Sdr,
                _ => OutputMode::Both,
            },
            input: InputOptions {
                white_nits: set(self.input_white),
            },
            hdr: HdrOptions {
                format: match self.hdr_format {
                    1 => HdrFormat::Avif,
                    2 => HdrFormat::UltraHdr,
                    _ => HdrFormat::Png,
                },
                avif: AvifOptions {
                    quality: self.avif_quality.clamp(1, 100),
                    speed: self.avif_speed.min(10),
                    full_color: self.avif_full_color,
                },
                peak_nits: set(self.hdr_peak),
                for_discord: self.for_discord,
            },
            sdr: SdrOptions {
                white_nits: set(self.sdr_white).unwrap_or(SdrOptions::default().white_nits),
                peak_nits: set(self.sdr_peak),
            },
            sdr_format: if self.sdr_jpeg {
                SdrFormat::Jpeg {
                    quality: self.jpeg_quality.clamp(1, 100),
                }
            } else {
                SdrFormat::Png
            },
            max_height: (self.max_height > 0).then_some(self.max_height),
            ..ConvertOptions::default()
        }
    }
}

/// What was read from an image file, and what was made from it.
#[wasm_bindgen]
pub struct Processed {
    width: u32,
    height: u32,
    format: String,
    hdr_input: bool,
    thumb_width: u32,
    thumb_height: u32,
    thumb: Vec<u8>,
    hdr: Option<Vec<u8>>,
    sdr: Option<Vec<u8>>,
}

#[wasm_bindgen]
impl Processed {
    /// Width of the image in pixels, before any scaling.
    #[wasm_bindgen(getter)]
    pub fn width(&self) -> u32 {
        self.width
    }

    #[wasm_bindgen(getter)]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The file format, such as "JPEG XR".
    #[wasm_bindgen(getter)]
    pub fn format(&self) -> String {
        self.format.clone()
    }

    /// Whether the image is HDR; 8-bit SDR inputs only get an SDR output.
    #[wasm_bindgen(getter)]
    pub fn hdr_input(&self) -> bool {
        self.hdr_input
    }

    #[wasm_bindgen(getter)]
    pub fn thumb_width(&self) -> u32 {
        self.thumb_width
    }

    #[wasm_bindgen(getter)]
    pub fn thumb_height(&self) -> u32 {
        self.thumb_height
    }

    /// The tone-mapped thumbnail as RGBA, row by row.
    pub fn take_thumb(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.thumb)
    }

    /// The HDR file, if one was made.
    pub fn take_hdr(&mut self) -> Option<Vec<u8>> {
        self.hdr.take()
    }

    /// The SDR file, if one was made.
    pub fn take_sdr(&mut self) -> Option<Vec<u8>> {
        self.sdr.take()
    }
}

/// Reads the image file in `bytes` and makes a thumbnail that fits
/// `thumb_width`×`thumb_height`. With `convert`, also makes the files
/// `settings` ask for.
#[wasm_bindgen]
pub fn process(
    bytes: &[u8],
    settings: &Settings,
    thumb_width: u32,
    thumb_height: u32,
    convert: bool,
) -> Result<Processed, JsError> {
    let options = settings.options();
    let format = InputFormat::sniff(bytes)
        .map_or("", InputFormat::name)
        .into();
    let image = input::decode(bytes, &options.input).map_err(error)?;
    let (thumb_width, thumb_height, thumb) = thumbnail(&image, &options, thumb_width, thumb_height);
    let mut processed = Processed {
        width: image.width,
        height: image.height,
        format,
        hdr_input: image.hdr,
        thumb_width,
        thumb_height,
        thumb,
        hdr: None,
        sdr: None,
    };
    if convert {
        let converted = convert_image(image, &options).map_err(error)?;
        processed.hdr = converted.hdr;
        processed.sdr = converted.sdr;
    }
    Ok(processed)
}

/// The SDR picture of `image` scaled to fit, as RGBA.
fn thumbnail(
    image: &ScRgbImage,
    options: &ConvertOptions,
    max_width: u32,
    max_height: u32,
) -> (u32, u32, Vec<u8>) {
    let preview = preview_image(image, max_width.max(1), max_height.max(1));
    let rgb = sdr_rgb8(&preview.image, &preview.levels, &options.sdr);
    let alpha = preview.image.alpha.as_deref();
    let rgba = rgb
        .as_chunks::<3>()
        .0
        .iter()
        .enumerate()
        .flat_map(|(i, &[r, g, b])| {
            let a = alpha.map_or(255, |a| (a[i].clamp(0.0, 1.0) * 255.0).round() as u8);
            [r, g, b, a]
        })
        .collect();
    (preview.image.width, preview.image.height, rgba)
}

fn error(e: hdrshot_core::Error) -> JsError {
    JsError::new(&e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_are_the_core_defaults() {
        let options = Settings::new().options();
        let defaults = ConvertOptions::default();
        assert_eq!(options.mode, defaults.mode);
        assert_eq!(options.hdr, defaults.hdr);
        assert_eq!(options.sdr, defaults.sdr);
        assert_eq!(options.sdr_format, defaults.sdr_format);
        assert_eq!(options.max_height, defaults.max_height);
        assert_eq!(options.input, defaults.input);
    }
}
