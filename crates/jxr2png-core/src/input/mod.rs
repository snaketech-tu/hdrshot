//! Reading the image formats jxr2png accepts.
//!
//! Every format is decoded to the same linear scRGB [`ScRgbImage`] that the
//! JPEG XR path produces, so all outputs work unchanged for every input.
//!
//! Display-referred files (JPEG XR, HDR PNG, PQ or HLG JPEG XL) carry absolute brightness.
//! Scene-referred files (OpenEXR, Radiance) only say how bright things are
//! relative to each other; [`InputOptions::white_nits`] sets the brightness
//! of their 1.0 when the file doesn't.

use std::path::Path;

use crate::color::SCRGB_WHITE_NITS;
use crate::decode::ScRgbImage;
use crate::{Error, Result};

mod jxl;
mod openexr;
mod png;
mod radiance;

/// The image formats that can be converted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputFormat {
    Jxr,
    Exr,
    Radiance,
    Png,
    Jxl,
}

impl InputFormat {
    pub const ALL: [Self; 5] = [Self::Jxr, Self::Exr, Self::Radiance, Self::Png, Self::Jxl];

    /// File extensions, lowercase and without the dot.
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Jxr => &["jxr", "wdp"],
            Self::Exr => &["exr"],
            Self::Radiance => &["hdr"],
            Self::Png => &["png"],
            Self::Jxl => &["jxl"],
        }
    }

    /// A short name for messages, such as "OpenEXR".
    pub fn name(self) -> &'static str {
        match self {
            Self::Jxr => "JPEG XR",
            Self::Exr => "OpenEXR",
            Self::Radiance => "Radiance HDR",
            Self::Png => "HDR PNG",
            Self::Jxl => "JPEG XL",
        }
    }

    /// The format a file name's extension suggests.
    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        Self::ALL
            .into_iter()
            .find(|f| f.extensions().contains(&ext.as_str()))
    }

    /// The format of a file, from its first bytes.
    pub fn sniff(bytes: &[u8]) -> Option<Self> {
        if bytes.starts_with(b"II\xbc") {
            Some(Self::Jxr)
        } else if bytes.starts_with(&[0x76, 0x2f, 0x31, 0x01]) {
            Some(Self::Exr)
        } else if bytes.starts_with(b"#?RADIANCE") || bytes.starts_with(b"#?RGBE") {
            Some(Self::Radiance)
        } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            Some(Self::Png)
        } else if bytes.starts_with(&[0xff, 0x0a]) || bytes.starts_with(b"\0\0\0\x0cJXL \r\n\x87\n")
        {
            Some(Self::Jxl)
        } else {
            None
        }
    }
}

/// Whether `path` has the extension of a format that can be converted.
pub fn is_supported(path: &Path) -> bool {
    InputFormat::from_path(path).is_some()
}

/// Settings for reading input files.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct InputOptions {
    /// Brightness, in nits, of 1.0 in scene-referred files (OpenEXR and
    /// Radiance). `None` uses the file's own value where it has one (the
    /// OpenEXR `whiteLuminance` attribute) and otherwise 80 nits, like
    /// scRGB, so renders come out as bright as a Windows HDR screenshot.
    pub white_nits: Option<f32>,
}

impl InputOptions {
    /// The factor that turns a scene-referred 1.0 into scRGB, given the
    /// file's own white luminance if it has one.
    fn scale(&self, file_white_nits: Option<f32>) -> f32 {
        self.white_nits
            .or(file_white_nits.filter(|w| w.is_finite() && *w > 0.0))
            .unwrap_or(SCRGB_WHITE_NITS)
            / SCRGB_WHITE_NITS
    }
}

/// Decodes an image file's bytes, in whichever supported format they are.
pub fn decode(bytes: &[u8], options: &InputOptions) -> Result<ScRgbImage> {
    match InputFormat::sniff(bytes) {
        Some(InputFormat::Jxr) => crate::decode::decode(bytes),
        Some(InputFormat::Exr) => openexr::decode(bytes, options),
        Some(InputFormat::Radiance) => radiance::decode(bytes, options),
        Some(InputFormat::Png) => png::decode(bytes),
        Some(InputFormat::Jxl) => jxl::decode(bytes, options),
        None => Err(Error::UnknownFormat),
    }
}

/// Reads and decodes an image file.
pub fn decode_file(path: &Path, options: &InputOptions) -> Result<ScRgbImage> {
    decode(&std::fs::read(path)?, options)
}

/// An image file's width and height, read without decoding the pixels
/// where the format allows.
pub fn dimensions(path: &Path) -> Result<(u32, u32)> {
    use std::io::Read;
    let mut head = Vec::new();
    std::fs::File::open(path)?
        .take(64 * 1024)
        .read_to_end(&mut head)?;
    let size = match InputFormat::sniff(&head) {
        Some(InputFormat::Jxr) => return crate::decode::dimensions(path),
        Some(InputFormat::Png) => png::dimensions(&head),
        Some(InputFormat::Radiance) => radiance::dimensions(&head),
        Some(InputFormat::Jxl) => jxl::dimensions(&head),
        Some(InputFormat::Exr) => None,
        None => return Err(Error::UnknownFormat),
    };
    match size {
        Some(size) => Ok(size),
        None => {
            let image = decode_file(path, &InputOptions::default())?;
            Ok((image.width, image.height))
        }
    }
}

/// Builds an image from pixels in linear RGB with `primaries`, scaling them
/// by `scale` to scRGB. Alpha is dropped if every pixel is opaque.
fn to_scrgb(
    width: u32,
    height: u32,
    mut pixels: Vec<[f32; 3]>,
    alpha: Option<Vec<f32>>,
    primaries: &crate::color::Primaries,
    scale: f32,
) -> ScRgbImage {
    use rayon::prelude::*;
    let mut m = crate::color::conversion_matrix(primaries, &crate::color::Primaries::BT709);
    for row in &mut m {
        for v in row {
            *v *= scale;
        }
    }
    pixels.par_iter_mut().for_each(|p| {
        *p = crate::color::apply_matrix(&m, *p).map(|c| if c.is_finite() { c } else { 0.0 });
    });
    ScRgbImage {
        width,
        height,
        pixels,
        alpha: alpha.filter(|a| a.iter().any(|&v| v < 1.0)),
        hdr: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_by_extension_and_content() {
        assert_eq!(
            InputFormat::from_path(Path::new("a/shot.JXR")),
            Some(InputFormat::Jxr)
        );
        assert_eq!(
            InputFormat::from_path(Path::new("sky.hdr")),
            Some(InputFormat::Radiance)
        );
        assert_eq!(InputFormat::from_path(Path::new("notes.txt")), None);
        assert_eq!(
            InputFormat::sniff(b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe"),
            Some(InputFormat::Radiance)
        );
        assert_eq!(InputFormat::sniff(b"GIF89a"), None);
        assert!(matches!(
            decode(b"not an image", &InputOptions::default()),
            Err(Error::UnknownFormat)
        ));
    }

    #[test]
    fn white_nits_option_overrides_the_file() {
        let default = InputOptions::default();
        assert_eq!(default.scale(None), 1.0);
        assert_eq!(default.scale(Some(160.0)), 2.0);
        let set = InputOptions {
            white_nits: Some(203.0),
        };
        assert_eq!(set.scale(Some(160.0)), 203.0 / 80.0);
    }
}
