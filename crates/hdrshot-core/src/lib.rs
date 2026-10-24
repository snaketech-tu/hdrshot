//! Conversion of HDR images to HDR and SDR PNG.
//!
//! Inputs are JPEG XR (`.jxr`, as made by Windows HDR screenshots), OpenEXR,
//! Radiance `.hdr`, HDR PNG and JPEG XL; see [`input`]. Two outputs are supported:
//! - **HDR**: 16-bit PNG in Rec.2020 primaries with the PQ (SMPTE ST 2084)
//!   transfer function, tagged with a `cICP` chunk and an ICC profile
//!   `cicp` tag so HDR-aware viewers display it in HDR.
//! - **SDR**: tone-mapped 8-bit sRGB PNG, or JPEG ([`SdrFormat`]).

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::SystemTime;

pub mod avif;
pub mod color;
pub mod decode;
pub mod encode;
pub mod icc;
pub mod input;
pub mod resize;
pub mod share;
pub mod ultrahdr;
pub mod watch;

pub use avif::{AvifOptions, write_hdr_avif};
use color::SCRGB_WHITE_NITS;
pub use decode::{ScRgbImage, decode, dimensions};
pub use encode::{
    LightLevels, for_discord, light_levels, sdr_rgb8, write_hdr_png, write_sdr_jpeg, write_sdr_png,
};
pub use input::{InputFormat, InputOptions, is_supported};
pub use ultrahdr::write_ultra_hdr;

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

/// File format of the HDR output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HdrFormat {
    /// 16-bit PNG, lossless; shown in HDR by Windows Photos and browsers.
    #[default]
    Png,
    /// 10-bit AVIF, about a twentieth of the PNG's size.
    Avif,
    /// SDR JPEG with a gain map (Ultra HDR): SDR in every viewer, HDR in
    /// those that read gain maps.
    UltraHdr,
}

impl HdrFormat {
    /// File extension, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Avif => "avif",
            Self::UltraHdr => "jpg",
        }
    }
}

/// Settings for the HDR output.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct HdrOptions {
    pub format: HdrFormat,
    /// Encoder settings when [`Self::format`] is AVIF.
    pub avif: AvifOptions,
    /// Optional brightest luminance, in nits, to write. Brighter highlights
    /// roll off smoothly up to this value. `None` (the default) keeps the
    /// screenshot's brightness exactly and leaves fitting it to the display
    /// to the viewer, as with the original JXR.
    pub peak_nits: Option<f32>,
    /// Write HDR PNGs without their `cICP` and `mDCV` chunks, as the
    /// Discord copy is (see [`for_discord`]), so that Discord shows the
    /// saved file itself correctly. Windows Photos and most HDR viewers
    /// need `cICP` and show such a PNG in SDR. Other formats ignore it.
    pub for_discord: bool,
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

/// File format of the SDR output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SdrFormat {
    /// 8-bit PNG, lossless.
    #[default]
    Png,
    /// Baseline JPEG of the given quality (1 to 100), a fraction of the
    /// PNG's size. Transparency is lost.
    Jpeg { quality: u8 },
}

impl SdrFormat {
    /// The JPEG quality used unless another is chosen.
    pub const DEFAULT_JPEG_QUALITY: u8 = 90;

    /// File extension, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg { .. } => "jpg",
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
    /// File name without extension. `{name}` is replaced by the input's name
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
    pub input: InputOptions,
    /// Directory to write into; `None` writes next to the input file.
    pub output_dir: Option<PathBuf>,
    pub naming: Naming,
    pub hdr: HdrOptions,
    pub sdr: SdrOptions,
    pub sdr_format: SdrFormat,
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
            input: InputOptions::default(),
            output_dir: None,
            naming: Naming::default(),
            hdr: HdrOptions::default(),
            sdr: SdrOptions::default(),
            sdr_format: SdrFormat::default(),
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
    #[error("could not decode image: {0}")]
    Decode(String),
    #[error("unsupported pixel format: {0}")]
    UnsupportedFormat(String),
    #[error("not a supported image (JPEG XR, OpenEXR, Radiance HDR, HDR PNG or JPEG XL)")]
    UnknownFormat,
    #[error("not an HDR image: {0}")]
    NotHdr(String),
    #[error("the output would replace the input file {}", .0.display())]
    SameAsInput(PathBuf),
    #[error("could not write PNG: {0}")]
    Encode(#[from] png::EncodingError),
    #[error("could not write AVIF: {0}")]
    EncodeAvif(String),
    #[error("could not write Ultra HDR JPEG: {0}")]
    EncodeUltraHdr(String),
    #[error("could not write JPEG: {0}")]
    EncodeJpeg(String),
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
    /// The input is an 8-bit SDR JPEG XR, so no HDR PNG was made even if one
    /// was asked for: it would only be the SDR picture in an HDR file.
    pub sdr_input: bool,
}

impl Outputs {
    /// Whether every output asked for was written: at least one, and none
    /// left alone because it already existed. Only then may the input be
    /// removed, as `--trash-originals` does.
    pub fn complete(&self) -> bool {
        (self.hdr.is_some() || self.sdr.is_some()) && self.skipped.is_empty()
    }
}

/// Whether `path` names a JPEG XR file (`.jxr` or `.wdp`).
pub fn is_jxr(path: &Path) -> bool {
    InputFormat::from_path(path) == Some(InputFormat::Jxr)
}

/// The images in `dir` to convert, sorted by path; with `recursive`, also
/// those in its subfolders. Unreadable folders are left out.
///
/// PNGs are not included, since a folder of converted images holds the
/// PNGs made from them; pass HDR PNGs by name instead.
pub fn find_inputs(dir: &Path, recursive: bool) -> Vec<PathBuf> {
    fn walk(dir: &Path, recursive: bool, seen: &mut HashSet<PathBuf>, out: &mut Vec<PathBuf>) {
        // A link back up the tree would otherwise be followed forever.
        if !seen.insert(dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf())) {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if recursive {
                    walk(&path, recursive, seen, out);
                }
            } else if InputFormat::from_path(&path).is_some_and(|f| f != InputFormat::Png) {
                out.push(path);
            }
        }
    }
    let mut found = Vec::new();
    walk(dir, recursive, &mut HashSet::new(), &mut found);
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
    let path = |kind: &str, extension: &str| {
        let name = naming
            .pattern
            .replace("{name}", &stem)
            .replace("{type}", kind);
        let dir = if naming.subfolders {
            dir.join(kind)
        } else {
            dir.clone()
        };
        dir.join(format!("{name}.{extension}"))
    };
    Outputs {
        hdr: options
            .mode
            .hdr()
            .then(|| path("hdr", options.hdr.format.extension())),
        sdr: options
            .mode
            .sdr()
            .then(|| path("sdr", options.sdr_format.extension())),
        ..Outputs::default()
    }
}

/// For each of `inputs`, converted together with `options`, an output file
/// that an earlier input also makes, and that input's index. Converting
/// both would leave only one of the two results, so callers leave such
/// inputs out and say why.
pub fn output_conflicts(
    inputs: &[PathBuf],
    options: &ConvertOptions,
) -> Vec<Option<(PathBuf, usize)>> {
    let mut makers: HashMap<PathBuf, usize> = HashMap::new();
    inputs
        .iter()
        .enumerate()
        .map(|(i, input)| {
            let planned = output_paths(input, options);
            let paths: Vec<PathBuf> = [planned.hdr, planned.sdr].into_iter().flatten().collect();
            let keys: Vec<PathBuf> = paths.iter().map(|p| file_key(p)).collect();
            if let Some((path, &earlier)) = paths
                .iter()
                .zip(&keys)
                .find_map(|(path, key)| Some((path, makers.get(key)?)))
            {
                return Some((path.clone(), earlier));
            }
            for key in keys {
                makers.insert(key, i);
            }
            None
        })
        .collect()
}

/// `path` spelled the same way whichever way it was given, as far as that
/// can be told before it exists: absolute, and in lowercase where file
/// names ignore case (Windows and macOS).
fn file_key(path: &Path) -> PathBuf {
    let dir = match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir,
        _ => Path::new("."),
    };
    let dir = dir
        .canonicalize()
        .or_else(|_| std::path::absolute(dir))
        .unwrap_or_else(|_| dir.to_path_buf());
    let key = dir.join(path.file_name().unwrap_or_default());
    if cfg!(any(windows, target_os = "macos")) {
        PathBuf::from(key.to_string_lossy().to_lowercase())
    } else {
        key
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

/// The temporary files being written, or `None` once
/// [`remove_unfinished_files`] has run.
static UNFINISHED: Mutex<Option<Vec<PathBuf>>> = Mutex::new(Some(Vec::new()));

/// Deletes the temporary files of the writes in progress and makes later
/// writes fail, so that a program stopped mid-conversion, such as with
/// Ctrl+C, leaves no temporary file next to its outputs. Call it right
/// before exiting.
pub fn remove_unfinished_files() {
    let mut unfinished = UNFINISHED.lock().unwrap_or_else(PoisonError::into_inner);
    for path in unfinished.take().unwrap_or_default() {
        let _ = std::fs::remove_file(path);
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
    // Unique per write, as files converted at the same time may write to
    // the same folder, or even to the same file.
    static WRITES: AtomicU64 = AtomicU64::new(0);
    let write_id = WRITES.fetch_add(1, Ordering::Relaxed);
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let temp = path.with_file_name(format!(".{name}.{}-{write_id}.tmp", std::process::id()));
    // Created and listed under the lock, so `remove_unfinished_files`
    // finds every temporary file.
    let file = {
        let mut unfinished = UNFINISHED.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(unfinished) = unfinished.as_mut() else {
            return Err(std::io::Error::from(std::io::ErrorKind::Interrupted).into());
        };
        let file = File::create(&temp)?;
        unfinished.push(temp.clone());
        file
    };
    let result = (|| {
        let mut writer = BufWriter::new(file);
        write(&mut writer)?;
        writer.flush()?;
        // Closed before the rename, which Windows needs.
        {
            let file = writer.into_inner().map_err(|e| e.into_error())?;
            if let Some(time) = modified {
                file.set_modified(time)?;
            }
        }
        std::fs::rename(&temp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    if let Some(unfinished) = UNFINISHED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_mut()
    {
        unfinished.retain(|p| *p != temp);
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
pub fn preview(
    input: &Path,
    options: &InputOptions,
    max_width: u32,
    max_height: u32,
) -> Result<Preview> {
    let image = input::decode_file(input, options)?;
    Ok(preview_image(&image, max_width, max_height))
}

/// Like [`preview`], for an image already decoded.
pub fn preview_image(image: &ScRgbImage, max_width: u32, max_height: u32) -> Preview {
    let levels = light_levels(image);
    let (width, height) = resize::fit_box(image.width, image.height, max_width, max_height);
    let image = resize::resize(image, width, height);
    Preview { image, levels }
}

/// The files made from one image by [`convert_bytes`], in memory.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Converted {
    pub hdr: Option<Vec<u8>>,
    pub sdr: Option<Vec<u8>>,
    /// The input is 8-bit SDR, so the SDR image was made in place of the
    /// HDR one, as [`Outputs::sdr_input`].
    pub sdr_input: bool,
}

/// Converts the image file in `bytes` as [`convert_file`] does, returning
/// the files instead of writing them. Used where there is no file system,
/// as in the browser version. The options about files (`output_dir`,
/// `naming`, `existing`, `keep_date`) don't apply.
pub fn convert_bytes(bytes: &[u8], options: &ConvertOptions) -> Result<Converted> {
    convert_image(input::decode(bytes, &options.input)?, options)
}

/// Like [`convert_bytes`], for an image already decoded with
/// `options.input`.
pub fn convert_image(image: ScRgbImage, options: &ConvertOptions) -> Result<Converted> {
    let sdr_input = !image.hdr;
    let make_hdr = options.mode.hdr() && !sdr_input;
    let make_sdr = options.mode.sdr() || (options.mode.hdr() && sdr_input);
    let image = scale_down(image, options);
    let levels = light_levels(&image);
    let mut converted = Converted {
        sdr_input,
        ..Converted::default()
    };
    if make_hdr {
        let mut out = Vec::new();
        write_hdr(&image, &levels, options, &mut out)?;
        converted.hdr = Some(out);
    }
    if make_sdr {
        let mut out = Vec::new();
        write_sdr(&image, &levels, options, &mut out)?;
        converted.sdr = Some(out);
    }
    Ok(converted)
}

/// `image` scaled down to [`ConvertOptions::max_height`], if it is taller.
fn scale_down(image: ScRgbImage, options: &ConvertOptions) -> ScRgbImage {
    match options.max_height {
        Some(max_height) => {
            let (width, height) = resize::fit_height(image.width, image.height, max_height);
            resize::resize(&image, width, height)
        }
        None => image,
    }
}

/// Encodes the HDR output of `image` in the format `options` ask for.
fn write_hdr(
    image: &ScRgbImage,
    levels: &LightLevels,
    options: &ConvertOptions,
    w: impl Write,
) -> Result<()> {
    match options.hdr.format {
        HdrFormat::Png => write_hdr_png(image, levels, &options.hdr, &options.sdr, w),
        HdrFormat::Avif => write_hdr_avif(image, &options.hdr, &options.hdr.avif, w),
        HdrFormat::UltraHdr => write_ultra_hdr(image, levels, &options.hdr, &options.sdr, w),
    }
}

/// Encodes the SDR output of `image` in the format `options` ask for.
fn write_sdr(
    image: &ScRgbImage,
    levels: &LightLevels,
    options: &ConvertOptions,
    w: impl Write,
) -> Result<()> {
    match options.sdr_format {
        SdrFormat::Png => write_sdr_png(image, levels, &options.sdr, w),
        SdrFormat::Jpeg { quality } => write_sdr_jpeg(image, levels, &options.sdr, quality, w),
    }
}

/// Convert one image file according to `options`.
pub fn convert_file(input: &Path, options: &ConvertOptions) -> Result<Outputs> {
    convert(input, options, false).map(|(outputs, _)| outputs)
}

/// Like [`convert_file`], and when the HDR output is an AVIF or Ultra HDR
/// JPEG, also returns the HDR PNG to copy to Discord in its place (see
/// [`share::discord_png`]), made from the same decoded image. Discord shows
/// AVIF too bright and Ultra HDR JPEGs in SDR only.
pub fn convert_file_with_discord_png(
    input: &Path,
    options: &ConvertOptions,
) -> Result<(Outputs, Option<Vec<u8>>)> {
    convert(input, options, options.hdr.format != HdrFormat::Png)
}

fn convert(
    input: &Path,
    options: &ConvertOptions,
    discord_png: bool,
) -> Result<(Outputs, Option<Vec<u8>>)> {
    let planned = output_paths(input, options);
    for path in [&planned.hdr, &planned.sdr].into_iter().flatten() {
        if same_file(path, input) {
            return Err(Error::SameAsInput(input.to_path_buf()));
        }
    }
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
        return Ok((outputs, None));
    }

    let bytes = std::fs::read(input)?;
    let modified = if options.keep_date {
        std::fs::metadata(input)?.modified().ok()
    } else {
        None
    };
    let image = input::decode(&bytes, &options.input)?;
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
            return Ok((outputs, None));
        }
    }
    let image = scale_down(image, options);
    let levels = light_levels(&image);
    for path in [&outputs.hdr, &outputs.sdr].into_iter().flatten() {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
    }
    if let Some(path) = &outputs.hdr {
        write_file(path, modified, |w| write_hdr(&image, &levels, options, w))?;
    }
    if let Some(path) = &outputs.sdr {
        write_file(path, modified, |w| write_sdr(&image, &levels, options, w))?;
    }
    let discord = if discord_png && outputs.hdr.is_some() {
        let mut png = Vec::new();
        write_hdr_png(&image, &levels, &options.hdr, &options.sdr, &mut png)?;
        Some(for_discord(&png))
    } else {
        None
    };
    Ok((outputs, discord))
}

/// Whether `a` and `b` name the same file, also when only one exists yet.
fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_only_when_everything_was_written() {
        let written = Outputs {
            hdr: Some("a_hdr.png".into()),
            sdr: Some("a_sdr.png".into()),
            ..Default::default()
        };
        assert!(written.complete());
        let partly = Outputs {
            hdr: Some("a_hdr.png".into()),
            skipped: vec!["a_sdr.png".into()],
            ..Default::default()
        };
        assert!(!partly.complete());
        assert!(!Outputs::default().complete());
    }

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
    fn hdr_extension_follows_format() {
        let opts = ConvertOptions {
            hdr: HdrOptions {
                format: HdrFormat::Avif,
                ..Default::default()
            },
            ..Default::default()
        };
        let out = output_paths(Path::new("shots/game.jxr"), &opts);
        assert_eq!(out.hdr, Some(PathBuf::from("shots/game_hdr.avif")));
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

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("hdrshot-lib")
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
    fn discord_png_only_for_avif_and_ultra_hdr() {
        let fixture = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/hdr64.jxr"
        ));
        for (format, expected) in [
            (HdrFormat::Png, false),
            (HdrFormat::Avif, true),
            (HdrFormat::UltraHdr, true),
        ] {
            let options = ConvertOptions {
                mode: OutputMode::Hdr,
                output_dir: Some(temp_dir(&format!("discord-{format:?}"))),
                hdr: HdrOptions {
                    format,
                    ..Default::default()
                },
                ..Default::default()
            };
            let (outputs, png) = convert_file_with_discord_png(fixture, &options).unwrap();
            assert!(outputs.hdr.is_some());
            assert_eq!(png.is_some(), expected, "{format:?}");
            if let Some(png) = png {
                assert!(png.starts_with(b"\x89PNG"));
                assert!(!png.windows(4).any(|w| w == b"cICP"));
            }
        }
    }

    #[test]
    fn refuses_to_replace_the_input() {
        let dir = temp_dir("same");
        let input = dir.join("shot.png");
        std::fs::write(&input, b"png").unwrap();
        let options = ConvertOptions {
            mode: OutputMode::Hdr,
            naming: Naming {
                pattern: "{name}".into(),
                subfolders: false,
            },
            existing: Existing::Replace,
            ..Default::default()
        };
        assert!(matches!(
            convert_file(&input, &options),
            Err(Error::SameAsInput(_))
        ));
        assert_eq!(std::fs::read(&input).unwrap(), b"png");
    }

    #[test]
    fn folder_scan_finds_inputs_but_not_pngs() {
        let dir = temp_dir("scan");
        for name in ["a.jxr", "b.EXR", "c.hdr", "d.png", "e.txt"] {
            std::fs::write(dir.join(name), b"").unwrap();
        }
        let names: Vec<_> = find_inputs(&dir, false)
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["a.jxr", "b.EXR", "c.hdr"]);
    }

    #[test]
    fn inputs_making_the_same_file_conflict() {
        let into = |dir: &str| ConvertOptions {
            output_dir: Some(PathBuf::from(dir)),
            ..Default::default()
        };
        let inputs: Vec<PathBuf> = ["d1/shot.jxr", "d2/shot.hdr", "d2/other.jxr", "shots/a.jxr"]
            .map(PathBuf::from)
            .into();
        assert_eq!(
            output_conflicts(&inputs, &into("out")),
            [
                None,
                Some((PathBuf::from("out/shot_hdr.png"), 0)),
                None,
                None
            ]
        );
        // Next to each input, only same-named files in one folder clash.
        let same_folder: Vec<PathBuf> = ["shots/a.jxr", "shots/a.exr", "other/a.jxr"]
            .map(PathBuf::from)
            .into();
        let conflicts = output_conflicts(&same_folder, &ConvertOptions::default());
        assert_eq!(
            conflicts,
            [None, Some((PathBuf::from("shots/a_hdr.png"), 0)), None]
        );
        // SDR only: the SDR PNG is what clashes.
        let sdr = ConvertOptions {
            mode: OutputMode::Sdr,
            ..into("out")
        };
        assert_eq!(
            output_conflicts(&inputs[..2], &sdr)[1],
            Some((PathBuf::from("out/shot_sdr.png"), 0))
        );
    }

    #[test]
    fn overlapping_writes_to_one_file_keep_apart() {
        // As when two conversions running at once make the same file.
        let dir = temp_dir("overlap");
        let path = dir.join("shot_hdr.png");
        write_file(&path, None, |outer| {
            outer.write_all(b"outer")?;
            write_file(&path, None, |inner| Ok(inner.write_all(b"inner")?))
        })
        .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"outer");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn folder_scan_survives_a_link_loop() {
        let dir = temp_dir("loop");
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub/a.jxr"), b"").unwrap();
        std::os::unix::fs::symlink(&dir, dir.join("sub/back")).unwrap();
        let found = find_inputs(&dir, true);
        assert_eq!(found, [dir.join("sub/a.jxr")]);
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
