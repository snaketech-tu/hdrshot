//! Conversion of HDR JPEG XR (`.jxr`) images to PNG.
//!
//! Two outputs are supported:
//! - **HDR**: 16-bit PNG in Rec.2020 primaries with the PQ (SMPTE ST 2084)
//!   transfer function, tagged with a `cICP` chunk and an ICC profile
//!   `cicp` tag so HDR-aware viewers display it in HDR.
//! - **SDR**: tone-mapped 8-bit sRGB PNG.

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

pub mod color;
pub mod decode;
pub mod encode;
pub mod icc;

use color::SCRGB_WHITE_NITS;
pub use decode::{ScRgbImage, decode};
pub use encode::{LightLevels, light_levels, without_cicp, write_hdr_png, write_sdr_png};

/// Which PNG files to produce for each input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputMode {
    Hdr,
    Sdr,
    #[default]
    Both,
}

impl OutputMode {
    pub fn hdr(self) -> bool {
        matches!(self, Self::Hdr | Self::Both)
    }

    pub fn sdr(self) -> bool {
        matches!(self, Self::Sdr | Self::Both)
    }
}

/// Settings for the HDR output.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct HdrOptions {
    /// Optional brightest luminance, in nits, to write. Brighter highlights
    /// roll off smoothly up to this value. `None` (the default) keeps the
    /// screenshot's brightness exactly and leaves fitting it to the display
    /// to the viewer, as with the original JXR.
    pub peak_nits: Option<f32>,
}

/// Settings for the tone-mapped SDR output.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SdrOptions {
    /// HDR luminance, in nits, that becomes SDR white. Defaults to 80,
    /// scRGB 1.0, which is how Windows shows these screenshots as SDR.
    pub white_nits: f32,
    /// Luminance, in nits, that maps to the SDR maximum. `None` uses the
    /// image's 99.9th percentile, so a few tiny specular pixels don't
    /// darken the whole picture.
    pub peak_nits: Option<f32>,
}

impl Default for SdrOptions {
    fn default() -> Self {
        Self {
            white_nits: SCRGB_WHITE_NITS,
            peak_nits: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ConvertOptions {
    pub mode: OutputMode,
    /// Directory to write into; `None` writes next to the input file.
    pub output_dir: Option<PathBuf>,
    pub hdr: HdrOptions,
    pub sdr: SdrOptions,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("could not decode JXR: {0}")]
    Decode(String),
    #[error("unsupported JXR pixel format: {0}")]
    UnsupportedFormat(String),
    #[error("could not write PNG: {0}")]
    Encode(#[from] png::EncodingError),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Paths of the files written for one input.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outputs {
    pub hdr: Option<PathBuf>,
    pub sdr: Option<PathBuf>,
}

/// Output paths that `convert_file` will write for `input`.
pub fn output_paths(input: &Path, options: &ConvertOptions) -> Outputs {
    let dir = options
        .output_dir
        .clone()
        .or_else(|| input.parent().map(Path::to_path_buf))
        .unwrap_or_default();
    let stem = input.file_stem().unwrap_or_default().to_string_lossy();
    let path = |suffix: &str| dir.join(format!("{stem}{suffix}.png"));
    Outputs {
        hdr: options.mode.hdr().then(|| path("_hdr")),
        sdr: options.mode.sdr().then(|| path("_sdr")),
    }
}

/// Convert one `.jxr` file according to `options`.
pub fn convert_file(input: &Path, options: &ConvertOptions) -> Result<Outputs> {
    let outputs = output_paths(input, options);
    let image = decode(&std::fs::read(input)?)?;
    let levels = light_levels(&image);
    if let Some(dir) = &options.output_dir {
        std::fs::create_dir_all(dir)?;
    }
    if let Some(path) = &outputs.hdr {
        write_hdr_png(
            &image,
            &levels,
            &options.hdr,
            &options.sdr,
            BufWriter::new(File::create(path)?),
        )?;
    }
    if let Some(path) = &outputs.sdr {
        write_sdr_png(
            &image,
            &levels,
            &options.sdr,
            BufWriter::new(File::create(path)?),
        )?;
    }
    Ok(outputs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_paths_next_to_input() {
        let opts = ConvertOptions::default();
        let out = output_paths(Path::new("shots/game.jxr"), &opts);
        assert_eq!(out.hdr, Some(PathBuf::from("shots/game_hdr.png")));
        assert_eq!(out.sdr, Some(PathBuf::from("shots/game_sdr.png")));
    }

    #[test]
    fn output_paths_respects_mode_and_dir() {
        let opts = ConvertOptions {
            mode: OutputMode::Sdr,
            output_dir: Some(PathBuf::from("out")),
            ..Default::default()
        };
        let out = output_paths(Path::new("shots/game.jxr"), &opts);
        assert_eq!(out.hdr, None);
        assert_eq!(out.sdr, Some(PathBuf::from("out/game_sdr.png")));
    }
}
