use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Mutex;

use clap::Parser;
use jxr2png_core::{ConvertOptions, Existing, HdrOptions, OutputMode, Outputs, SdrOptions};
use rayon::prelude::*;

/// Convert HDR JPEG XR (.jxr) images to HDR and/or SDR PNG files.
#[derive(Parser, Debug)]
#[command(version, about)]
struct Args {
    /// Input .jxr files or folders. Wildcards such as *.jxr also work in
    /// cmd and PowerShell.
    #[arg(required = true)]
    inputs: Vec<PathBuf>,

    /// Also convert .jxr files in subfolders of the given folders.
    #[arg(short, long)]
    recursive: bool,

    /// Write the HDR PNG (16-bit Rec.2020 PQ).
    #[arg(long)]
    hdr: bool,

    /// Write the tone-mapped SDR PNG (8-bit sRGB).
    #[arg(long)]
    sdr: bool,

    /// Output directory (defaults to each input's directory).
    #[arg(short, long)]
    output: Option<PathBuf>,

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

    /// Scale images taller than this down to this height in pixels, keeping
    /// the aspect ratio (e.g. 1080). By default the source resolution is
    /// kept.
    #[arg(long, value_name = "PIXELS")]
    max_height: Option<u32>,

    /// Replace PNGs that already exist. By default they are skipped.
    #[arg(long)]
    overwrite: bool,

    /// Give the PNGs the current time instead of the JXR's modified time.
    #[arg(long)]
    no_keep_date: bool,

    /// Print only errors.
    #[arg(short, long, conflicts_with = "json")]
    quiet: bool,

    /// Print one JSON object per input file instead of text.
    #[arg(long)]
    json: bool,
}

/// How many files to convert at once. Decoding and PNG compression are
/// mostly single-threaded, so files in parallel are much faster, but each
/// 4K capture needs a few hundred MB while it's converted.
const MAX_PARALLEL_FILES: usize = 8;

fn main() -> ExitCode {
    // Windows shells pass wildcards through unexpanded; `wild` expands them.
    let args = Args::parse_from(wild::args_os());
    let mode = match (args.hdr, args.sdr) {
        (true, false) => OutputMode::Hdr,
        (false, true) => OutputMode::Sdr,
        _ => OutputMode::Both,
    };
    let options = ConvertOptions {
        mode,
        output_dir: args.output.clone(),
        hdr: HdrOptions {
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

    let mut missing = false;
    let mut files = Vec::new();
    for input in &args.inputs {
        if input.is_dir() {
            let found = jxr2png_core::find_jxr(input, args.recursive);
            if found.is_empty() && !args.quiet {
                eprintln!("{}: no .jxr files found", input.display());
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
    pool.install(|| {
        files.par_iter().with_max_len(1).for_each(|input| {
            let result = jxr2png_core::convert_file(input, &options);
            report.file(input, result);
        });
    });
    let failed = report.finish();
    if failed > 0 || missing {
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
    fn json_escapes() {
        assert_eq!(json(r"C:\shots\a.jxr"), r#""C:\\shots\\a.jxr""#);
        assert_eq!(json("say \"hi\"\n"), r#""say \"hi\"\n""#);
        assert_eq!(json("\u{1}"), r#""\u0001""#);
    }
}
