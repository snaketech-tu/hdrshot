use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use jxr2png_core::{ConvertOptions, HdrOptions, OutputMode, SdrOptions};

/// Convert HDR JPEG XR (.jxr) images to HDR and/or SDR PNG files.
#[derive(Parser, Debug)]
#[command(version, about)]
struct Args {
    /// Input .jxr files.
    #[arg(required = true)]
    inputs: Vec<PathBuf>,

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
}

fn main() -> ExitCode {
    let args = Args::parse();
    let mode = match (args.hdr, args.sdr) {
        (true, false) => OutputMode::Hdr,
        (false, true) => OutputMode::Sdr,
        _ => OutputMode::Both,
    };
    let options = ConvertOptions {
        mode,
        output_dir: args.output,
        hdr: HdrOptions {
            peak_nits: args.hdr_peak,
        },
        sdr: SdrOptions {
            white_nits: args.sdr_white,
            peak_nits: args.sdr_peak,
        },
        max_height: args.max_height,
    };

    let mut failed = false;
    for input in &args.inputs {
        match jxr2png_core::convert_file(input, &options) {
            Ok(outputs) => {
                for path in [outputs.hdr, outputs.sdr].into_iter().flatten() {
                    println!("{} -> {}", input.display(), path.display());
                }
            }
            Err(e) => {
                eprintln!("{}: {e}", input.display());
                failed = true;
            }
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
