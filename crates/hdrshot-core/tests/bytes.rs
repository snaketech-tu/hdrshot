//! `convert_bytes`, used by the browser version, must make the same files
//! as `convert_file`.

use std::path::{Path, PathBuf};

use hdrshot_core::{
    ConvertOptions, Converted, HdrFormat, HdrOptions, OutputMode, SdrFormat, convert_bytes,
    convert_file,
};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

/// A fresh output folder for one test.
fn out_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("hdrshot-bytes")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Converts `fixture` both ways and returns the in-memory result and the
/// files `convert_file` wrote.
fn both_ways(fixture: &str, name: &str, options: ConvertOptions) -> (Converted, Converted) {
    let input = Path::new(FIXTURES).join(fixture);
    let converted = convert_bytes(&std::fs::read(&input).unwrap(), &options).unwrap();
    let outputs = convert_file(
        &input,
        &ConvertOptions {
            output_dir: Some(out_dir(name)),
            ..options
        },
    )
    .unwrap();
    let read = |path: Option<PathBuf>| path.map(|p| std::fs::read(p).unwrap());
    let written = Converted {
        hdr: read(outputs.hdr),
        sdr: read(outputs.sdr),
        sdr_input: outputs.sdr_input,
    };
    (converted, written)
}

#[test]
fn same_files_as_convert_file() {
    let cases = [
        ("png", ConvertOptions::default()),
        (
            "avif",
            ConvertOptions {
                hdr: HdrOptions {
                    format: HdrFormat::Avif,
                    ..Default::default()
                },
                ..Default::default()
            },
        ),
        (
            "ultrahdr-jpeg",
            ConvertOptions {
                hdr: HdrOptions {
                    format: HdrFormat::UltraHdr,
                    ..Default::default()
                },
                sdr_format: SdrFormat::Jpeg { quality: 85 },
                max_height: Some(32),
                ..Default::default()
            },
        ),
    ];
    for (name, options) in cases {
        let (converted, written) = both_ways("hdr64.jxr", name, options);
        assert!(converted.hdr.is_some() && converted.sdr.is_some(), "{name}");
        assert_eq!(converted, written, "{name}");
    }
}

#[test]
fn sdr_input_makes_the_sdr_image_instead() {
    let options = ConvertOptions {
        mode: OutputMode::Hdr,
        ..Default::default()
    };
    let (converted, written) = both_ways("sdr32.jxr", "sdr-input", options);
    assert!(converted.sdr_input);
    assert!(converted.hdr.is_none() && converted.sdr.is_some());
    assert_eq!(converted, written);
}

#[test]
fn unknown_bytes_are_an_error() {
    assert!(convert_bytes(b"not an image", &ConvertOptions::default()).is_err());
}
