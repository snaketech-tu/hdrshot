//! Reading the image formats hdrshot accepts.
//!
//! Every format is decoded to the same linear scRGB [`ScRgbImage`] that the
//! JPEG XR path produces, so all outputs work unchanged for every input.
//!
//! Display-referred files (JPEG XR, HDR PNG, PQ or HLG JPEG XL, HEIF and AVIF) carry
//! absolute brightness. Scene-referred files (OpenEXR, Radiance) only say how
//! bright things are relative to each other, and gain map photos (phone
//! JPEGs, HEICs and AVIFs) how bright they are relative to SDR white;
//! [`InputOptions::white_nits`] sets the brightness of their 1.0 when the
//! file doesn't.

use std::path::Path;

use crate::color::SCRGB_WHITE_NITS;
use crate::decode::ScRgbImage;
use crate::{Error, Result};

#[cfg(not(target_arch = "wasm32"))]
mod av1;
#[cfg(not(target_arch = "wasm32"))]
mod avif;
mod gainmap;
mod heif;
mod isobmff;
mod jpeg;
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
    /// A JPEG with a gain map, as phones save HDR photos.
    Jpeg,
    /// HEIC, as iPhones and cameras save photos.
    Heif,
    /// AVIF in PQ or HLG, or with a gain map. Not available in the browser
    /// version, where its decoder doesn't build.
    Avif,
}

impl InputFormat {
    #[cfg(not(target_arch = "wasm32"))]
    pub const ALL: [Self; 8] = [
        Self::Jxr,
        Self::Exr,
        Self::Radiance,
        Self::Png,
        Self::Jxl,
        Self::Jpeg,
        Self::Heif,
        Self::Avif,
    ];
    #[cfg(target_arch = "wasm32")]
    pub const ALL: [Self; 7] = [
        Self::Jxr,
        Self::Exr,
        Self::Radiance,
        Self::Png,
        Self::Jxl,
        Self::Jpeg,
        Self::Heif,
    ];

    /// File extensions, lowercase and without the dot.
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Jxr => &["jxr", "wdp"],
            Self::Exr => &["exr"],
            Self::Radiance => &["hdr"],
            Self::Png => &["png"],
            Self::Jxl => &["jxl"],
            Self::Jpeg => &["jpg", "jpeg"],
            Self::Heif => &["heic", "heif", "hif"],
            Self::Avif => &["avif"],
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
            Self::Jpeg => "HDR JPEG",
            Self::Heif => "HEIC",
            Self::Avif => "HDR AVIF",
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
        } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
            Some(Self::Jpeg)
        } else if heif::sniff(bytes) {
            Some(Self::Heif)
        } else if isobmff::brands(bytes).iter().any(|b| b == b"avif") {
            Some(Self::Avif)
        } else {
            None
        }
    }
}

/// Whether `path` has the extension of a format that can be converted.
pub fn is_supported(path: &Path) -> bool {
    InputFormat::from_path(path).is_some()
}

/// Whether a folder scan or the watch folder should pick up `path`.
///
/// PNGs, JPEGs and AVIFs are what hdrshot writes, so a folder of converted
/// images holds them too. PNGs are left out. JPEGs are read up to their
/// image data and included only if they look like phone HDR photos with a
/// gain map and weren't written by hdrshot. AVIFs are included only with a
/// gain map (the `tmap` brand), which hdrshot's AVIFs don't have.
pub fn is_scan_input(path: &Path) -> bool {
    use std::io::Read;
    let head = |len: u64| {
        let mut head = Vec::new();
        std::fs::File::open(path)
            .and_then(|f| f.take(len).read_to_end(&mut head))
            .map(|_| head)
    };
    match InputFormat::from_path(path) {
        None | Some(InputFormat::Png) => false,
        Some(InputFormat::Jpeg) => head(256 * 1024).is_ok_and(|h| jpeg::has_gain_map(&h)),
        Some(InputFormat::Avif) => {
            head(256).is_ok_and(|h| isobmff::brands(&h).iter().any(|b| b == b"tmap"))
        }
        Some(_) => true,
    }
}

/// Whether XMP says hdrshot wrote the file.
fn made_by_hdrshot(xmp: &str) -> bool {
    xmp.contains("x:xmptk=\"hdrshot\"")
}

/// Settings for reading input files.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct InputOptions {
    /// Brightness, in nits, of 1.0 in scene-referred files (OpenEXR and
    /// Radiance). `None` uses the file's own value where it has one (the
    /// OpenEXR `whiteLuminance` attribute) and otherwise 80 nits, like
    /// scRGB, so renders come out as bright as a Windows HDR screenshot.
    ///
    /// For gain map photos it is the brightness of SDR white, by default
    /// 203 nits, the reference white of HDR video and photos (ITU-R BT.2408).
    /// Ultra HDR JPEGs written by hdrshot use 80 nits, as hdrshot made them.
    pub white_nits: Option<f32>,
}

/// SDR white of gain map photos, in nits (ITU-R BT.2408, ISO 22028-5).
pub const GAIN_MAP_WHITE_NITS: f32 = 203.0;

impl InputOptions {
    /// The factor that turns a scene-referred 1.0 into scRGB, given the
    /// file's own white luminance if it has one.
    fn scale(&self, file_white_nits: Option<f32>) -> f32 {
        self.white_nits
            .or(file_white_nits.filter(|w| w.is_finite() && *w > 0.0))
            .unwrap_or(SCRGB_WHITE_NITS)
            / SCRGB_WHITE_NITS
    }

    /// Nits of SDR white in a gain map photo.
    fn gain_map_white(&self, made_by_hdrshot: bool) -> f32 {
        self.white_nits
            .filter(|w| w.is_finite() && *w > 0.0)
            .unwrap_or(if made_by_hdrshot {
                SCRGB_WHITE_NITS
            } else {
                GAIN_MAP_WHITE_NITS
            })
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
        Some(InputFormat::Jpeg) => jpeg::decode(bytes, options),
        Some(InputFormat::Heif) => heif::decode(bytes, options),
        #[cfg(not(target_arch = "wasm32"))]
        Some(InputFormat::Avif) => avif::decode(bytes, options),
        #[cfg(target_arch = "wasm32")]
        Some(InputFormat::Avif) => Err(Error::UnsupportedFormat(
            "AVIF input isn't available in the browser version".into(),
        )),
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
        Some(InputFormat::Exr) => openexr::dimensions(path),
        Some(InputFormat::Jpeg) => jpeg::dimensions(&head),
        Some(InputFormat::Heif) => heif::dimensions(&head).or_else(|| {
            // The boxes describing the image may come after the first bytes.
            heif::dimensions(&std::fs::read(path).ok()?)
        }),
        #[cfg(not(target_arch = "wasm32"))]
        Some(InputFormat::Avif) => {
            avif::dimensions(&head).or_else(|| avif::dimensions(&std::fs::read(path).ok()?))
        }
        #[cfg(target_arch = "wasm32")]
        Some(InputFormat::Avif) => None,
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
        sdr_white: None,
    }
}

/// The primaries of an RGB ICC profile, from its colorant tags.
fn icc_primaries(icc: &[u8]) -> Option<crate::color::Primaries> {
    let u32_at = |at: usize| Some(u32::from_be_bytes(icc.get(at..at + 4)?.try_into().ok()?));
    if icc.get(16..20)? != b"RGB " {
        return None;
    }
    let tag = |name: &[u8; 4]| -> Option<[f64; 3]> {
        let count = u32_at(128)? as usize;
        let (offset, _) = (0..count.min(100)).find_map(|i| {
            let entry = 132 + 12 * i;
            (icc.get(entry..entry + 4)? == name)
                .then(|| Some((u32_at(entry + 4)?, u32_at(entry + 8)?)))?
        })?;
        let offset = offset as usize;
        if icc.get(offset..offset + 4)? != b"XYZ " {
            return None;
        }
        let fixed = |at: usize| Some(f64::from(u32_at(at)? as i32) / 65536.0);
        Some([fixed(offset + 8)?, fixed(offset + 12)?, fixed(offset + 16)?])
    };
    let xy = |xyz: [f64; 3]| -> Option<[f64; 2]> {
        let sum = xyz[0] + xyz[1] + xyz[2];
        (sum > 0.0).then(|| [xyz[0] / sum, xyz[1] / sum])
    };
    let (r, g, b) = (tag(b"rXYZ")?, tag(b"gXYZ")?, tag(b"bXYZ")?);
    let white = xy([r[0] + g[0] + b[0], r[1] + g[1] + b[1], r[2] + g[2] + b[2]])?;
    // Colorants are adapted to the D50 white of the profile connection space.
    Some(crate::color::Primaries {
        red: xy(r)?,
        green: xy(g)?,
        blue: xy(b)?,
        white,
    })
}

/// An EXIF orientation: how to turn the stored picture to show it upright.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Orientation {
    /// Quarter turns clockwise, after mirroring.
    turns: u8,
    mirror: bool,
}

impl Orientation {
    fn from_exif(value: u8) -> Self {
        let (turns, mirror) = match value {
            2 => (0, true),
            3 => (2, false),
            4 => (2, true),
            5 => (3, true),
            6 => (1, false),
            7 => (1, true),
            8 => (3, false),
            _ => (0, false),
        };
        Self { turns, mirror }
    }

    /// Whether width and height trade places.
    fn swaps(self) -> bool {
        self.turns % 2 == 1
    }

    /// Turns a `width` by `height` picture upright.
    fn apply<T: Copy + Send + Sync>(
        self,
        pixels: Vec<T>,
        width: u32,
        height: u32,
    ) -> (Vec<T>, u32, u32) {
        use rayon::prelude::*;
        if self.turns == 0 && !self.mirror {
            return (pixels, width, height);
        }
        let (w, h) = (width as usize, height as usize);
        let (out_w, out_h) = if self.swaps() { (h, w) } else { (w, h) };
        let out = (0..out_w * out_h)
            .into_par_iter()
            .map(|i| {
                let (x, y) = (i % out_w, i / out_w);
                // Undo the turn, then the mirror.
                let (sx, sy) = match self.turns {
                    1 => (y, h - 1 - x),
                    2 => (w - 1 - x, h - 1 - y),
                    3 => (w - 1 - y, x),
                    _ => (x, y),
                };
                let sx = if self.mirror { w - 1 - sx } else { sx };
                pixels[sy * w + sx]
            })
            .collect();
        (out, out_w as u32, out_h as u32)
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
        assert_eq!(default.gain_map_white(false), 203.0);
        assert_eq!(default.gain_map_white(true), 80.0);
        assert_eq!(set.gain_map_white(true), 203.0);
    }

    #[test]
    fn orientation_turns_the_picture() {
        // 0 1 2
        // 3 4 5
        let pixels: Vec<u8> = (0..6).collect();
        let turn = |v: u8| Orientation::from_exif(v).apply(pixels.clone(), 3, 2);
        assert_eq!(turn(1), (pixels.clone(), 3, 2));
        assert_eq!(turn(6), (vec![3, 0, 4, 1, 5, 2], 2, 3));
        assert_eq!(turn(8), (vec![2, 5, 1, 4, 0, 3], 2, 3));
        assert_eq!(turn(3), (vec![5, 4, 3, 2, 1, 0], 3, 2));
        assert_eq!(turn(2), (vec![2, 1, 0, 5, 4, 3], 3, 2));
        assert_eq!(turn(5), (vec![0, 3, 1, 4, 2, 5], 2, 3));
        assert_eq!(turn(7), (vec![5, 2, 4, 1, 3, 0], 2, 3));
    }
}
