//! Conversion of HDR JPEG XR (`.jxr`) images to PNG.
//!
//! Two outputs are supported:
//! - **HDR**: 16-bit PNG in Rec.2020 primaries with the PQ (SMPTE ST 2084)
//!   transfer function, tagged with a `cICP` chunk and an ICC profile
//!   `cicp` tag so HDR-aware viewers display it in HDR.
//! - **SDR**: tone-mapped 8-bit sRGB PNG.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub mod color;
pub mod decode;
pub mod encode;
pub mod icc;
pub mod resize;
pub mod share;

use color::SCRGB_WHITE_NITS;
pub use decode::{ScRgbImage, decode, dimensions};
pub use encode::{LightLevels, for_discord, light_levels, sdr_rgb8, write_hdr_png, write_sdr_png};

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

/// What to do when an output file already exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Existing {
    /// Leave the existing file alone and don't write that output.
    #[default]
    Skip,
    /// Replace the existing file.
    Replace,
    /// Write to a new name, e.g. `shot_hdr (2).png`.
    KeepBoth,
}

/// How output files are named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Naming {
    /// File name without `.png`. `{name}` is replaced by the input's name
    /// without extension, `{type}` by `hdr` or `sdr`.
    pub pattern: String,
    /// Put HDR and SDR PNGs into `hdr` and `sdr` subfolders.
    pub subfolders: bool,
}

impl Naming {
    pub const DEFAULT_PATTERN: &str = "{name}_{type}";

    /// Why `self` can't name the outputs of `mode`, if it can't.
    pub fn check(&self, mode: OutputMode) -> std::result::Result<(), String> {
        if !self.pattern.contains("{name}") {
            return Err("the name pattern needs {name}, or every file gets the same name".into());
        }
        if self.pattern.contains(['/', '\\']) {
            return Err("the name pattern can't contain folders; use the output folder".into());
        }
        if mode == OutputMode::Both && !self.subfolders && !self.pattern.contains("{type}") {
            return Err(
                "with both HDR and SDR PNGs in one folder, the name pattern needs {type}".into(),
            );
        }
        Ok(())
    }
}

impl Default for Naming {
    fn default() -> Self {
        Self {
            pattern: Self::DEFAULT_PATTERN.into(),
            subfolders: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConvertOptions {
    pub mode: OutputMode,
    /// Directory to write into; `None` writes next to the input file.
    pub output_dir: Option<PathBuf>,
    pub naming: Naming,
    pub hdr: HdrOptions,
    pub sdr: SdrOptions,
    /// Scale images taller than this down to this height, keeping the
    /// aspect ratio. `None` (the default) keeps the source resolution.
    pub max_height: Option<u32>,
    pub existing: Existing,
    /// Give the PNGs the modified time of the JXR, so folders sorted by
    /// date stay in capture order. On by default.
    pub keep_date: bool,
}

impl Default for ConvertOptions {
    fn default() -> Self {
        Self {
            mode: OutputMode::default(),
            output_dir: None,
            naming: Naming::default(),
            hdr: HdrOptions::default(),
            sdr: SdrOptions::default(),
            max_height: None,
            existing: Existing::default(),
            keep_date: true,
        }
    }
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
    /// Outputs that already existed and were left alone
    /// ([`Existing::Skip`]).
    pub skipped: Vec<PathBuf>,
    /// The input is an 8-bit SDR image, so no HDR PNG was made even if one
    /// was asked for: it would only be the SDR picture in an HDR file.
    pub sdr_input: bool,
}

/// Whether `path` names a JPEG XR file (`.jxr` or `.wdp`).
pub fn is_jxr(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("jxr") || e.eq_ignore_ascii_case("wdp"))
}

/// The JPEG XR files in `dir`, sorted by path; with `recursive`, also those
/// in its subfolders. Unreadable folders are left out.
pub fn find_jxr(dir: &Path, recursive: bool) -> Vec<PathBuf> {
    fn walk(dir: &Path, recursive: bool, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if recursive {
                    walk(&path, recursive, out);
                }
            } else if is_jxr(&path) {
                out.push(path);
            }
        }
    }
    let mut found = Vec::new();
    walk(dir, recursive, &mut found);
    found.sort();
    found
}

/// Output paths that `convert_file` will write for `input`, before
/// [`ConvertOptions::existing`] is applied.
pub fn output_paths(input: &Path, options: &ConvertOptions) -> Outputs {
    let dir = options
        .output_dir
        .clone()
        .or_else(|| input.parent().map(Path::to_path_buf))
        .unwrap_or_default();
    let stem = input.file_stem().unwrap_or_default().to_string_lossy();
    let naming = &options.naming;
    let path = |kind: &str| {
        let name = naming
            .pattern
            .replace("{name}", &stem)
            .replace("{type}", kind);
        let dir = if naming.subfolders {
            dir.join(kind)
        } else {
            dir.clone()
        };
        dir.join(format!("{name}.png"))
    };
    Outputs {
        hdr: options.mode.hdr().then(|| path("hdr")),
        sdr: options.mode.sdr().then(|| path("sdr")),
        ..Outputs::default()
    }
}

/// Applies `existing` to a planned output path: the path to write, or
/// `None` to skip it.
fn resolve_existing(path: PathBuf, existing: Existing) -> Option<PathBuf> {
    if !path.exists() {
        return Some(path);
    }
    match existing {
        Existing::Skip => None,
        Existing::Replace => Some(path),
        Existing::KeepBoth => {
            let stem = path.file_stem().unwrap_or_default().to_string_lossy();
            let ext = path.extension().unwrap_or_default().to_string_lossy();
            (2..)
                .map(|n| path.with_file_name(format!("{stem} ({n}).{ext}")))
                .find(|candidate| !candidate.exists())
        }
    }
}

/// Writes `path` through a temporary file in the same folder that is renamed
/// into place only once complete, so a failed conversion never leaves a
/// broken PNG behind.
fn write_file(
    path: &Path,
    modified: Option<SystemTime>,
    write: impl FnOnce(&mut BufWriter<File>) -> Result<()>,
) -> Result<()> {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let temp = path.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    let result = (|| {
        let mut writer = BufWriter::new(File::create(&temp)?);
        write(&mut writer)?;
        writer.flush()?;
        let file = writer.into_inner().map_err(|e| e.into_error())?;
        if let Some(time) = modified {
            file.set_modified(time)?;
        }
        drop(file);
        std::fs::rename(&temp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

/// A small copy of an image for showing it, with the light levels of the
/// full image so it tone-maps like the PNG ([`sdr_rgb8`]).
#[derive(Debug, Clone)]
pub struct Preview {
    pub image: ScRgbImage,
    pub levels: LightLevels,
}

/// Decodes `input` and scales it to fit `max_width`×`max_height`.
pub fn preview(input: &Path, max_width: u32, max_height: u32) -> Result<Preview> {
    let image = decode(&std::fs::read(input)?)?;
    let levels = light_levels(&image);
    let (width, height) = resize::fit_box(image.width, image.height, max_width, max_height);
    let image = resize::resize(&image, width, height);
    Ok(Preview { image, levels })
}

/// Convert one `.jxr` file according to `options`.
pub fn convert_file(input: &Path, options: &ConvertOptions) -> Result<Outputs> {
    let planned = output_paths(input, options);
    let mut outputs = Outputs::default();
    for (planned, resolved) in [
        (planned.hdr, &mut outputs.hdr),
        (planned.sdr, &mut outputs.sdr),
    ] {
        if let Some(path) = planned {
            *resolved = resolve_existing(path.clone(), options.existing);
            if resolved.is_none() {
                outputs.skipped.push(path);
            }
        }
    }
    if outputs.hdr.is_none() && outputs.sdr.is_none() {
        return Ok(outputs);
    }

    let bytes = std::fs::read(input)?;
    let modified = if options.keep_date {
        std::fs::metadata(input)?.modified().ok()
    } else {
        None
    };
    let mut image = decode(&bytes)?;
    drop(bytes);
    if !image.hdr {
        outputs.sdr_input = true;
        if outputs.hdr.take().is_some() && !options.mode.sdr() {
            // HDR only was asked for: make the SDR PNG instead of nothing.
            let sdr = output_paths(
                input,
                &ConvertOptions {
                    mode: OutputMode::Sdr,
                    ..options.clone()
                },
            )
            .sdr;
            outputs.sdr = sdr.and_then(|path| {
                let resolved = resolve_existing(path.clone(), options.existing);
                if resolved.is_none() {
                    outputs.skipped.push(path);
                }
                resolved
            });
        }
        if outputs.sdr.is_none() {
            return Ok(outputs);
        }
    }
    if let Some(max_height) = options.max_height {
        let (width, height) = resize::fit_height(image.width, image.height, max_height);
        image = resize::resize(&image, width, height);
    }
    let levels = light_levels(&image);
    for path in [&outputs.hdr, &outputs.sdr].into_iter().flatten() {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
    }
    if let Some(path) = &outputs.hdr {
        write_file(path, modified, |w| {
            write_hdr_png(&image, &levels, &options.hdr, &options.sdr, w)
        })?;
    }
    if let Some(path) = &outputs.sdr {
        write_file(path, modified, |w| {
            write_sdr_png(&image, &levels, &options.sdr, w)
        })?;
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
    fn output_paths_follow_naming() {
        let both = |pattern: &str, subfolders| {
            let opts = ConvertOptions {
                naming: Naming {
                    pattern: pattern.into(),
                    subfolders,
                },
                ..Default::default()
            };
            let out = output_paths(Path::new("shots/game.jxr"), &opts);
            (out.hdr.unwrap(), out.sdr.unwrap())
        };
        assert_eq!(
            both("{type}-{name}", false),
            ("shots/hdr-game.png".into(), "shots/sdr-game.png".into())
        );
        assert_eq!(
            both("{name}", true),
            ("shots/hdr/game.png".into(), "shots/sdr/game.png".into())
        );
    }

    #[test]
    fn naming_check() {
        let naming = |pattern: &str, subfolders| Naming {
            pattern: pattern.into(),
            subfolders,
        };
        assert!(Naming::default().check(OutputMode::Both).is_ok());
        assert!(naming("{name}", false).check(OutputMode::Hdr).is_ok());
        assert!(naming("{name}", true).check(OutputMode::Both).is_ok());
        assert!(naming("{name}", false).check(OutputMode::Both).is_err());
        assert!(
            naming("shot_{type}", false)
                .check(OutputMode::Both)
                .is_err()
        );
        assert!(naming("out/{name}", false).check(OutputMode::Sdr).is_err());
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

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("jxr2png-lib")
            .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn resolve_existing_per_policy() {
        let dir = temp_dir("existing");
        let free = dir.join("free_hdr.png");
        assert_eq!(resolve_existing(free.clone(), Existing::Skip), Some(free));

        let taken = dir.join("shot_hdr.png");
        std::fs::write(&taken, b"old").unwrap();
        std::fs::write(dir.join("shot_hdr (2).png"), b"old").unwrap();
        assert_eq!(resolve_existing(taken.clone(), Existing::Skip), None);
        assert_eq!(
            resolve_existing(taken.clone(), Existing::Replace),
            Some(taken.clone())
        );
        assert_eq!(
            resolve_existing(taken, Existing::KeepBoth),
            Some(dir.join("shot_hdr (3).png"))
        );
    }

    #[test]
    fn failed_write_leaves_nothing_behind() {
        let dir = temp_dir("failed");
        let path = dir.join("shot_hdr.png");
        let result = write_file(&path, None, |w| {
            w.write_all(b"half a PNG")?;
            Err(Error::Decode("simulated failure".into()))
        });
        assert!(result.is_err());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
    }

    #[test]
    fn write_sets_modified_time() {
        let dir = temp_dir("modified");
        let path = dir.join("shot_sdr.png");
        let time = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        write_file(&path, Some(time), |w| Ok(w.write_all(b"png")?)).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"png");
        assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), time);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    }
}
