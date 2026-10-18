//! Conversion of HDR JPEG XR (`.jxr`) images to PNG.
//!
//! Two outputs are supported:
//! - **HDR**: 16-bit PNG in Rec.2020 primaries with the PQ (SMPTE ST 2084)
//!   transfer function, tagged with a `cICP` chunk so HDR-aware viewers
//!   display it in HDR.
//! - **SDR**: tone-mapped 8-bit sRGB PNG.

use std::path::{Path, PathBuf};

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

#[derive(Debug, Clone, Default)]
pub struct ConvertOptions {
    pub mode: OutputMode,
    /// Directory to write into; `None` writes next to the input file.
    pub output_dir: Option<PathBuf>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JXR decoding is not implemented yet")]
    Unimplemented,
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
    let _ = output_paths(input, options);
    Err(Error::Unimplemented)
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
        };
        let out = output_paths(Path::new("shots/game.jxr"), &opts);
        assert_eq!(out.hdr, None);
        assert_eq!(out.sdr, Some(PathBuf::from("out/game_sdr.png")));
    }
}
