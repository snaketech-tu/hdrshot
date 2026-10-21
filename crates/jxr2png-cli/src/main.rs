use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Mutex;

use clap::{CommandFactory, Parser};
use jxr2png_core::{
    AvifOptions, ConvertOptions, Existing, HdrFormat, HdrOptions, InputOptions, Naming, OutputMode,
    Outputs, SdrOptions,
};
use rayon::prelude::*;

/// Convert HDR images (JPEG XR, OpenEXR, Radiance HDR, HDR PNG, JPEG XL)
/// to HDR and/or SDR PNG files.
#[derive(Parser, Debug)]
#[command(version, about)]
struct Args {
    /// Input images or folders. Wildcards such as *.jxr also work in cmd and
    /// PowerShell. Folders are searched for .jxr, .exr, .hdr and .jxl files; give
    /// HDR PNGs by name.
    #[arg(required = true)]
    inputs: Vec<PathBuf>,

    /// Also convert images in subfolders of the given folders.
    #[arg(short, long)]
    recursive: bool,

    /// Write the HDR PNG (16-bit Rec.2020 PQ).
    #[arg(long)]
    hdr: bool,

    /// Write the tone-mapped SDR PNG (8-bit sRGB).
    #[arg(long)]
    sdr: bool,

    /// File format of the HDR output: png (16-bit, lossless), avif (10-bit,
    /// about a twentieth of the size) or ultrahdr (a JPEG that is SDR in
    /// every viewer and HDR in those that read gain maps).
    #[arg(long, value_enum, default_value_t = Format::Png)]
    format: Format,

    /// AVIF quality from 1 to 100; higher is closer to the original and
    /// larger.
    #[arg(long, value_name = "1-100", default_value_t = AvifOptions::default().quality,
          value_parser = clap::value_parser!(u8).range(1..=100))]
    avif_quality: u8,

    /// AVIF encoder speed from 0 (slowest, smallest file) to 10 (fastest).
    #[arg(long, value_name = "0-10", default_value_t = AvifOptions::default().speed,
          value_parser = clap::value_parser!(u8).range(0..=10))]
    avif_speed: u8,

    /// Store AVIF color at half resolution (4:2:0): smaller, but colored
    /// text and thin colored edges get softer.
    #[arg(long)]
    avif_420: bool,

    /// Output directory (defaults to each input's directory).
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Output file name without extension: {name} is the input's name,
    /// {type} is hdr or sdr.
    #[arg(long, value_name = "PATTERN", default_value = Naming::DEFAULT_PATTERN)]
    name: String,

    /// Put HDR and SDR PNGs into hdr and sdr subfolders.
    #[arg(long)]
    subfolders: bool,

    /// Limit the HDR PNG's brightest highlight to this many nits; brighter
    /// parts roll off smoothly. By default the original brightness is kept.
    #[arg(long, value_name = "NITS")]
    hdr_peak: Option<f32>,

    /// HDR brightness, in nits, that becomes white in the SDR PNG.
    #[arg(long, value_name = "NITS", default_value_t = SdrOptions::default().white_nits)]
    sdr_white: f32,

    /// HDR brightness, in nits, that maps to the SDR maximum
    /// (defaults to the image's 99.9th percentile).
    #[arg(long, value_name = "NITS")]
    sdr_peak: Option<f32>,

    /// Brightness, in nits, of 1.0 in OpenEXR, Radiance and float JPEG XL
    /// files. By default
    /// the file's own value is used, or 80 nits if it has none.
    #[arg(long, value_name = "NITS")]
    input_white: Option<f32>,

    /// Scale images taller than this down to this height in pixels, keeping
    /// the aspect ratio (e.g. 1080). By default the source resolution is
    /// kept.
    #[arg(long, value_name = "PIXELS")]
    max_height: Option<u32>,

    /// Replace PNGs that already exist. By default they are skipped.
    #[arg(long)]
    overwrite: bool,

    /// Give the PNGs the current time instead of the input's modified time.
    #[arg(long)]
    no_keep_date: bool,

    /// Print only errors.
    #[arg(short, long, conflicts_with = "json")]
    quiet: bool,

    /// Print one JSON object per input file instead of text.
    #[arg(long)]
    json: bool,

    /// Copy the last file's PNG (the HDR one if made) to the clipboard, to
    /// paste into Discord or a folder. The copy leaves out the cICP chunk,
    /// which Discord turns yellow.
    #[arg(long)]
    copy: bool,

    /// Like --copy, but if the PNG is over Discord's 10 MB limit, copy it
    /// at 1440p or lower so it fits. The saved PNGs stay as they are.
    #[arg(long)]
    copy_small: bool,
}

// Internal commands, kept out of `Args` so they don't show up in the help,
// completions or man page. Each is the first argument, followed by one
// value.

/// `--hold-clipboard <PNG>`: keeps a copied file on the Linux clipboard,
/// which only holds it while a program offers it.
const HOLD_CLIPBOARD: &str = "--hold-clipboard";
/// `--generate <WHAT>`: prints a shell completion script (bash, zsh, fish,
/// powershell, elvish) or the man page (man), for packaging.
const GENERATE: &str = "--generate";

#[derive(clap::ValueEnum, Clone, Copy, Debug)]
enum Format {
    Png,
    Avif,
    #[value(name = "ultrahdr")]
    UltraHdr,
}

#[derive(clap::ValueEnum, Clone, Copy, Debug)]
enum Generate {
    Bash,
    Zsh,
    Fish,
    Powershell,
    Elvish,
    Man,
}

/// Writes what `what` asks for to `out`.
fn generate(what: Generate, out: &mut dyn Write) -> std::io::Result<()> {
    use clap_complete::Shell;
    let mut command = Args::command();
    let shell = match what {
        Generate::Bash => Shell::Bash,
        Generate::Zsh => Shell::Zsh,
        Generate::Fish => Shell::Fish,
        Generate::Powershell => Shell::PowerShell,
        Generate::Elvish => Shell::Elvish,
        Generate::Man => {
            return clap_mangen::Man::new(command).render(out);
        }
    };
    clap_complete::generate(shell, &mut command, "jxr2png", out);
    Ok(())
}

/// How many files to convert at once. Decoding and PNG compression are
/// mostly single-threaded, so files in parallel are much faster, but each
/// 4K capture needs a few hundred MB while it's converted.
const MAX_PARALLEL_FILES: usize = 8;

fn main() -> ExitCode {
    // Windows shells pass wildcards through unexpanded; `wild` expands them.
    let raw: Vec<std::ffi::OsString> = wild::args_os().collect();
    if let [_, command, value] = &raw[..] {
        if command == HOLD_CLIPBOARD {
            return clipboard::hold(Path::new(value));
        }
        if command == GENERATE {
            use clap::ValueEnum;
            let what = value
                .to_str()
                .and_then(|v| Generate::from_str(v, true).ok());
            let Some(what) = what else {
                eprintln!("{GENERATE}: unknown value {}", value.display());
                return ExitCode::from(2);
            };
            return match generate(what, &mut std::io::stdout()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("{e}");
                    ExitCode::FAILURE
                }
            };
        }
    }
    let args = Args::parse_from(raw);
    let mode = match (args.hdr, args.sdr) {
        (true, false) => OutputMode::Hdr,
        (false, true) => OutputMode::Sdr,
        _ => OutputMode::Both,
    };
    let options = ConvertOptions {
        mode,
        input: InputOptions {
            white_nits: args.input_white,
        },
        output_dir: args.output.clone(),
        naming: Naming {
            pattern: args.name.clone(),
            subfolders: args.subfolders,
        },
        hdr: HdrOptions {
            format: match args.format {
                Format::Png => HdrFormat::Png,
                Format::Avif => HdrFormat::Avif,
                Format::UltraHdr => HdrFormat::UltraHdr,
            },
            avif: AvifOptions {
                quality: args.avif_quality,
                speed: args.avif_speed,
                full_color: !args.avif_420,
            },
            peak_nits: args.hdr_peak,
        },
        sdr: SdrOptions {
            white_nits: args.sdr_white,
            peak_nits: args.sdr_peak,
        },
        max_height: args.max_height,
        existing: if args.overwrite {
            Existing::Replace
        } else {
            Existing::Skip
        },
        keep_date: !args.no_keep_date,
    };
    if let Err(e) = options.naming.check(mode) {
        eprintln!("--name: {e}");
        return ExitCode::from(2);
    }

    let mut missing = false;
    let mut files = Vec::new();
    for input in &args.inputs {
        if input.is_dir() {
            let found = jxr2png_core::find_inputs(input, args.recursive);
            if found.is_empty() && !args.quiet {
                eprintln!("{}: no images found", input.display());
            }
            files.extend(found);
        } else if input.exists() {
            files.push(input.clone());
        } else {
            report_error(&args, input, "file not found");
            missing = true;
        }
    }

    let report = Report::new(&args, files.len());
    let threads = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .min(MAX_PARALLEL_FILES);
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .expect("thread pool");
    let results: Vec<Option<Outputs>> = pool.install(|| {
        files
            .par_iter()
            .with_max_len(1)
            .map(|input| {
                let result = jxr2png_core::convert_file(input, &options);
                let outputs = result.as_ref().ok().cloned();
                report.file(input, result);
                outputs
            })
            .collect()
    });
    let failed = report.finish();
    let mut copy_failed = false;
    if args.copy || args.copy_small {
        let last = files
            .iter()
            .zip(&results)
            .rev()
            .find_map(|(input, outputs)| {
                Some((input, clipboard::pick(input, outputs.as_ref()?, &options)?))
            });
        let message = match last {
            Some((input, (png, hdr))) => {
                clipboard::copy(input, &png, hdr, &options, args.copy_small)
            }
            None => Err("nothing to copy".into()),
        };
        match message {
            Ok(message) if !args.quiet && !args.json => eprintln!("{message}"),
            Ok(_) => {}
            Err(e) => {
                eprintln!("--copy: {e}");
                copy_failed = true;
            }
        }
    }
    if failed > 0 || missing || copy_failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn report_error(args: &Args, input: &Path, error: &str) {
    if args.json {
        println!(
            "{{\"input\":{},\"error\":{}}}",
            json(&input.display().to_string()),
            json(error)
        );
    } else {
        eprintln!("{}: {error}", input.display());
    }
}

/// Prints results as files finish, a progress line on a terminal, and a
/// summary.
struct Report<'a> {
    args: &'a Args,
    total: usize,
    progress: bool,
    state: Mutex<(usize, usize)>,
}

impl<'a> Report<'a> {
    fn new(args: &'a Args, total: usize) -> Self {
        Self {
            args,
            total,
            progress: total > 1 && !args.quiet && !args.json && std::io::stderr().is_terminal(),
            state: Mutex::new((0, 0)),
        }
    }

    fn file(&self, input: &Path, result: jxr2png_core::Result<Outputs>) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let (done, failed) = &mut *state;
        *done += 1;
        self.clear_progress();
        match result {
            Ok(outputs) => self.print_outputs(input, &outputs),
            Err(e) => {
                *failed += 1;
                report_error(self.args, input, &e.to_string());
            }
        }
        if self.progress && *done < self.total {
            eprint!("[{done}/{}] converting...", self.total);
            let _ = std::io::stderr().flush();
        }
    }

    fn print_outputs(&self, input: &Path, outputs: &Outputs) {
        let written = [&outputs.hdr, &outputs.sdr].into_iter().flatten();
        if self.args.json {
            let list = |paths: &mut dyn Iterator<Item = &PathBuf>| {
                let items: Vec<String> = paths.map(|p| json(&p.display().to_string())).collect();
                format!("[{}]", items.join(","))
            };
            println!(
                "{{\"input\":{},\"written\":{},\"skipped\":{},\"sdr_input\":{}}}",
                json(&input.display().to_string()),
                list(&mut written.into_iter()),
                list(&mut outputs.skipped.iter()),
                outputs.sdr_input
            );
        } else if !self.args.quiet {
            if outputs.sdr_input && !(self.args.sdr && !self.args.hdr) {
                println!(
                    "{}: an SDR image, so only the SDR PNG is made",
                    input.display()
                );
            }
            for path in written {
                println!("{} -> {}", input.display(), path.display());
            }
            for path in &outputs.skipped {
                println!(
                    "{}: skipped {}, which already exists (--overwrite replaces it)",
                    input.display(),
                    path.display()
                );
            }
        }
    }

    fn clear_progress(&self) {
        if self.progress {
            eprint!("\r\x1b[2K");
        }
    }

    /// Prints the summary and returns how many files failed.
    fn finish(self) -> usize {
        let (_, failed) = self.state.into_inner().unwrap_or_else(|e| e.into_inner());
        if failed > 0 && !self.args.json {
            eprintln!("{failed} of {} files failed", self.total);
        } else if self.total > 1 && !self.args.quiet && !self.args.json {
            eprintln!("Converted {} files", self.total);
        }
        failed
    }
}

mod clipboard {
    use std::path::{Path, PathBuf};
    use std::process::ExitCode;

    use jxr2png_core::share::{self, DISCORD_LIMIT, megabytes};
    use jxr2png_core::{ConvertOptions, Outputs};

    /// The image of `input` to copy, preferring the HDR one, and whether it
    /// is HDR. A PNG skipped because it already existed counts too.
    pub fn pick(
        input: &Path,
        outputs: &Outputs,
        options: &ConvertOptions,
    ) -> Option<(PathBuf, bool)> {
        let planned = jxr2png_core::output_paths(input, options);
        let made = |written: &Option<PathBuf>, planned: Option<PathBuf>| {
            written
                .clone()
                .or(planned.filter(|p| outputs.skipped.contains(p)))
        };
        let hdr = (!outputs.sdr_input)
            .then(|| made(&outputs.hdr, planned.hdr))
            .flatten();
        match hdr {
            Some(hdr) => Some((hdr, true)),
            None => made(&outputs.sdr, planned.sdr).map(|sdr| (sdr, false)),
        }
    }

    /// Copies `png`, made from `input`, to the clipboard; with `small`, a
    /// version that fits Discord's limit. Returns the message to print.
    pub fn copy(
        input: &Path,
        png: &Path,
        hdr: bool,
        options: &ConvertOptions,
        small: bool,
    ) -> Result<String, String> {
        let name = png.file_name().unwrap_or_default().to_string_lossy();
        let bytes = std::fs::read(png).map_err(|e| format!("{}: {e}", png.display()))?;
        let size = if hdr {
            jxr2png_core::for_discord(&bytes).len()
        } else {
            bytes.len()
        } as u64;
        let is_png = png
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("png"));
        let (bytes, message) = if size <= DISCORD_LIMIT {
            (bytes, format!("Copied {name} to the clipboard"))
        } else if small && is_png {
            let smaller = share::smaller_png(input, options, hdr, DISCORD_LIMIT)
                .map_err(|e| e.to_string())?
                .ok_or("even a 720p copy is larger than 10 MB")?;
            let message = format!(
                "Copied {name} at {}×{} ({}) to the clipboard",
                smaller.width,
                smaller.height,
                megabytes(smaller.png.len() as u64)
            );
            (smaller.png, message)
        } else {
            let message = format!(
                "Copied {name} to the clipboard; at {} it is over Discord's 10 MB limit \
                 (--copy-small copies a smaller version)",
                megabytes(size)
            );
            (bytes, message)
        };
        let file = share::clipboard_file(&name, &bytes, hdr).map_err(|e| e.to_string())?;
        set(&file)?;
        Ok(message)
    }

    /// Windows and macOS keep the file on the clipboard after jxr2png exits.
    #[cfg(not(target_os = "linux"))]
    fn set(file: &Path) -> Result<(), String> {
        arboard::Clipboard::new()
            .and_then(|mut c| c.set().file_list(&[file]))
            .map_err(|e| e.to_string())
    }

    /// On Linux the clipboard only holds what a running program offers, so
    /// a background copy of jxr2png offers the file until something else
    /// is copied.
    #[cfg(target_os = "linux")]
    fn set(file: &Path) -> Result<(), String> {
        use std::process::{Command, Stdio};
        // Fails fast when there is no display to copy to.
        arboard::Clipboard::new().map_err(|e| e.to_string())?;
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        Command::new(exe)
            .arg(super::HOLD_CLIPBOARD)
            .arg(file)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(drop)
            .map_err(|e| e.to_string())
    }

    /// Offers `file` on the clipboard until something else is copied.
    pub fn hold(file: &Path) -> ExitCode {
        #[cfg(target_os = "linux")]
        {
            use arboard::SetExtLinux;
            let held =
                arboard::Clipboard::new().and_then(|mut c| c.set().wait().file_list(&[file]));
            if held.is_err() {
                return ExitCode::FAILURE;
            }
        }
        #[cfg(not(target_os = "linux"))]
        let _ = file;
        ExitCode::SUCCESS
    }
}

/// `s` as a JSON string literal.
fn json(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_are_consistent() {
        Args::command().debug_assert();
    }

    #[test]
    fn generates_completions_and_man_page() {
        for (what, expect) in [
            (Generate::Bash, "--copy-small"),
            (Generate::Powershell, "--copy-small"),
            (Generate::Man, ".TH jxr2png"),
        ] {
            let mut out = Vec::new();
            generate(what, &mut out).unwrap();
            let text = String::from_utf8(out).unwrap();
            assert!(text.contains(expect), "{what:?}");
            assert!(!text.contains("hold-clipboard"), "{what:?}");
        }
    }

    #[test]
    fn json_escapes() {
        assert_eq!(json(r"C:\shots\a.jxr"), r#""C:\\shots\\a.jxr""#);
        assert_eq!(json("say \"hi\"\n"), r#""say \"hi\"\n""#);
        assert_eq!(json("\u{1}"), r#""\u0001""#);
    }
}
