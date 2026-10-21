//! What `convert_file` does with outputs that already exist, and with the
//! capture date.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use hdrshot_core::{ConvertOptions, Existing, OutputMode, convert_file};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/hdr64.jxr");

/// A fresh folder holding a copy of the fixture with a known modified time.
fn setup(name: &str) -> (PathBuf, PathBuf, SystemTime) {
    let dir = std::env::temp_dir()
        .join("hdrshot-existing")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let input = dir.join("shot.jxr");
    std::fs::copy(FIXTURE, &input).unwrap();
    let captured = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    std::fs::File::options()
        .write(true)
        .open(&input)
        .unwrap()
        .set_modified(captured)
        .unwrap();
    (dir, input, captured)
}

fn options(existing: Existing) -> ConvertOptions {
    ConvertOptions {
        mode: OutputMode::Hdr,
        existing,
        ..Default::default()
    }
}

fn modified(path: &Path) -> SystemTime {
    std::fs::metadata(path).unwrap().modified().unwrap()
}

#[test]
fn keeps_capture_date_by_default() {
    let (dir, input, captured) = setup("date");
    let outputs = convert_file(&input, &ConvertOptions::default()).unwrap();
    assert_eq!(modified(&outputs.hdr.unwrap()), captured);
    assert_eq!(modified(&outputs.sdr.unwrap()), captured);
    // Only the input and the two PNGs: no temporary files left over.
    assert_eq!(std::fs::read_dir(dir).unwrap().count(), 3);
}

#[test]
fn keep_date_off_uses_the_current_time() {
    let (_dir, input, captured) = setup("no-date");
    let outputs = convert_file(
        &input,
        &ConvertOptions {
            keep_date: false,
            ..options(Existing::Skip)
        },
    )
    .unwrap();
    assert!(modified(&outputs.hdr.unwrap()) > captured);
}

#[test]
fn skip_leaves_existing_files_alone() {
    let (dir, input, _) = setup("skip");
    let existing = dir.join("shot_hdr.png");
    std::fs::write(&existing, b"keep me").unwrap();
    let outputs = convert_file(&input, &options(Existing::Skip)).unwrap();
    assert_eq!(outputs.hdr, None);
    assert_eq!(outputs.skipped, vec![existing.clone()]);
    assert_eq!(std::fs::read(existing).unwrap(), b"keep me");
}

#[test]
fn skip_writes_only_the_missing_output() {
    let (dir, input, _) = setup("skip-one");
    std::fs::write(dir.join("shot_hdr.png"), b"keep me").unwrap();
    let outputs = convert_file(
        &input,
        &ConvertOptions {
            mode: OutputMode::Both,
            ..options(Existing::Skip)
        },
    )
    .unwrap();
    assert_eq!(outputs.hdr, None);
    assert_eq!(outputs.sdr, Some(dir.join("shot_sdr.png")));
    assert_eq!(outputs.skipped, vec![dir.join("shot_hdr.png")]);
}

#[test]
fn replace_overwrites() {
    let (dir, input, _) = setup("replace");
    let existing = dir.join("shot_hdr.png");
    std::fs::write(&existing, b"old").unwrap();
    let outputs = convert_file(&input, &options(Existing::Replace)).unwrap();
    assert_eq!(outputs.hdr, Some(existing.clone()));
    assert!(std::fs::read(existing).unwrap().starts_with(b"\x89PNG"));
}

#[test]
fn keep_both_picks_a_new_name() {
    let (dir, input, _) = setup("keep-both");
    std::fs::write(dir.join("shot_hdr.png"), b"old").unwrap();
    let outputs = convert_file(&input, &options(Existing::KeepBoth)).unwrap();
    assert_eq!(outputs.hdr, Some(dir.join("shot_hdr (2).png")));
    assert_eq!(std::fs::read(dir.join("shot_hdr.png")).unwrap(), b"old");
}

#[test]
fn failed_conversion_leaves_no_png() {
    let (dir, input, _) = setup("broken");
    let bytes = std::fs::read(&input).unwrap();
    std::fs::write(&input, &bytes[..bytes.len() / 2]).unwrap();
    assert!(convert_file(&input, &options(Existing::Replace)).is_err());
    assert_eq!(std::fs::read_dir(dir).unwrap().count(), 1);
}
