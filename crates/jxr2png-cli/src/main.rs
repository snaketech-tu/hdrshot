use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use jxr2png_core::{ConvertOptions, OutputMode};

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
    };

    let mut failed = false;
    for input in &args.inputs {
        match jxr2png_core::convert_file(input, &options) {
            Ok(_) => println!("{}: done", input.display()),
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
