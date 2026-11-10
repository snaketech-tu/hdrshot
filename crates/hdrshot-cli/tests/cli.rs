//! The command-line tool as users run it: options, outputs, messages and
//! exit codes, on the small fixtures of hdrshot-core.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../hdrshot-core/tests/fixtures")
}

/// An empty folder of its own for each test.
fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("hdrshot-cli")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// `dir` with the HDR screenshot fixture copied in as `name`.
fn with_shot(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::copy(fixtures().join("hdr64.jxr"), &path).unwrap();
    path
}

fn hdrshot(args: &[&std::ffi::OsStr]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_hdrshot"))
        .args(args)
        .output()
        .unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Names of the files in `dir`, sorted.
fn files(dir: &Path) -> Vec<String> {
    let mut names: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

macro_rules! args {
    ($($a:expr),* $(,)?) => { &[$(std::ffi::OsStr::new($a)),*][..] };
}

#[test]
fn converts_to_hdr_and_sdr_png() {
    let dir = temp_dir("both");
    let shot = with_shot(&dir, "shot.jxr");
    let out = hdrshot(args![&shot]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(files(&dir), ["shot.jxr", "shot_hdr.png", "shot_sdr.png"]);
    let stdout = text(&out.stdout);
    assert!(
        stdout.contains("shot_hdr.png") && stdout.contains("shot_sdr.png"),
        "{stdout}"
    );
    // The outputs keep the screenshot's date.
    let modified = |p: &Path| std::fs::metadata(p).unwrap().modified().unwrap();
    assert_eq!(modified(&dir.join("shot_hdr.png")), modified(&shot));
}

#[test]
fn hdr_or_sdr_only_and_other_formats() {
    let dir = temp_dir("formats");
    let shot = with_shot(&dir, "shot.jxr");
    let out_dir = dir.join("out");
    let out_str = out_dir.to_str().unwrap();
    for (extra, made) in [
        (&["--hdr"][..], "shot_hdr.png"),
        (&["--sdr"][..], "shot_sdr.png"),
        (&["--hdr", "--format", "avif"][..], "shot_hdr.avif"),
        (&["--hdr", "--format", "ultrahdr"][..], "shot_hdr.jpg"),
        (&["--sdr", "--sdr-format", "jpeg"][..], "shot_sdr.jpg"),
    ] {
        let _ = std::fs::remove_dir_all(&out_dir);
        let mut args: Vec<&std::ffi::OsStr> = vec!["-o".as_ref(), out_str.as_ref()];
        args.extend(extra.iter().map(|a| std::ffi::OsStr::new(*a)));
        args.push(shot.as_os_str());
        let out = hdrshot(&args);
        assert_eq!(
            out.status.code(),
            Some(0),
            "{extra:?}: {}",
            text(&out.stderr)
        );
        assert_eq!(files(&out_dir), [made], "{extra:?}");
    }
}

#[test]
fn existing_files_are_skipped_unless_overwrite() {
    let dir = temp_dir("existing");
    let shot = with_shot(&dir, "shot.jxr");
    let hdr = dir.join("shot_hdr.png");
    std::fs::write(&hdr, b"keep me").unwrap();

    let out = hdrshot(args![&shot]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(
        text(&out.stdout).contains("skipped"),
        "{}",
        text(&out.stdout)
    );
    assert_eq!(std::fs::read(&hdr).unwrap(), b"keep me");
    assert!(dir.join("shot_sdr.png").exists());

    let out = hdrshot(args!["--overwrite", &shot]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(std::fs::read(&hdr).unwrap().starts_with(b"\x89PNG"));
}

#[test]
fn folders_are_searched_for_images() {
    let dir = temp_dir("folder");
    with_shot(&dir, "a.jxr");
    std::fs::create_dir(dir.join("sub")).unwrap();
    with_shot(&dir.join("sub"), "b.jxr");

    let out = hdrshot(args!["--sdr", &dir]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(dir.join("a_sdr.png").exists());
    assert!(!dir.join("sub/b_sdr.png").exists());

    // A second run finds the PNGs it wrote but leaves them alone, and
    // `-r` goes into subfolders.
    let out = hdrshot(args!["--sdr", "-r", &dir]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(dir.join("sub/b_sdr.png").exists());
    assert!(!dir.join("a_sdr_sdr.png").exists());
}

#[test]
fn json_lines_for_results_and_errors() {
    let dir = temp_dir("json");
    let shot = with_shot(&dir, "shot.jxr");
    let missing = dir.join("missing.jxr");
    let out = hdrshot(args!["--json", "--sdr", &shot, &missing]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out.stderr));
    let lines: Vec<serde_json::Value> = text(&out.stdout)
        .lines()
        .map(|l| serde_json::from_str(l).expect(l))
        .collect();
    assert_eq!(lines.len(), 2, "{lines:?}");
    let ok = lines.iter().find(|l| l.get("written").is_some()).unwrap();
    assert!(ok["input"].as_str().unwrap().ends_with("shot.jxr"));
    assert!(ok["written"][0].as_str().unwrap().ends_with("shot_sdr.png"));
    assert_eq!(ok["sdr_input"], false);
    let failed = lines.iter().find(|l| l.get("error").is_some()).unwrap();
    assert!(failed["input"].as_str().unwrap().ends_with("missing.jxr"));
}

#[test]
fn sdr_input_gets_only_sdr() {
    let dir = temp_dir("sdr-input");
    let shot = dir.join("sdr.jxr");
    std::fs::copy(fixtures().join("sdr32.jxr"), &shot).unwrap();
    let out = hdrshot(args!["--json", &shot]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let line: serde_json::Value = serde_json::from_str(text(&out.stdout).trim()).unwrap();
    assert_eq!(line["sdr_input"], true);
    assert_eq!(files(&dir), ["sdr.jxr", "sdr_sdr.png"]);
}

#[test]
fn exit_codes() {
    let dir = temp_dir("exit");
    let shot = with_shot(&dir, "shot.jxr");
    // Not found.
    let out = hdrshot(args![&dir.join("nope.jxr")]);
    assert_eq!(out.status.code(), Some(1));
    // Not an image.
    let junk = dir.join("junk.jxr");
    std::fs::write(&junk, b"not an image").unwrap();
    let out = hdrshot(args!["--sdr", &junk, &shot]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("1 of 2 files failed"),
        "{}",
        text(&out.stderr)
    );
    assert!(dir.join("shot_sdr.png").exists());
    assert!(!dir.join("junk_sdr.png").exists());
    // Invalid arguments.
    for bad in [
        args!["--name", "{type}", &shot],
        args!["--quiet", "--json", &shot],
        args!["--sdr-white", "0", &shot],
        args!["--avif-quality", "101", &shot],
        args!["--no-such-option", &shot],
        args![],
    ] {
        let out = hdrshot(bad);
        assert_eq!(out.status.code(), Some(2), "{bad:?}: {}", text(&out.stderr));
    }
}

#[test]
fn custom_names_and_subfolders() {
    let dir = temp_dir("names");
    let shot = with_shot(&dir, "shot.jxr");
    let out_dir = dir.join("out");
    let out = hdrshot(args![
        "--name",
        "{name}-{type}",
        "--subfolders",
        "-o",
        &out_dir,
        &shot
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(out_dir.join("hdr/shot-hdr.png").exists());
    assert!(out_dir.join("sdr/shot-sdr.png").exists());
}

#[test]
fn watch_converts_new_images() {
    let dir = temp_dir("watch");
    with_shot(&dir, "old.jxr");
    let mut child = Command::new(env!("CARGO_BIN_EXE_hdrshot"))
        .args(["--sdr", "--watch"])
        .arg(&dir)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // It says when it is watching.
    let mut stderr = BufReader::new(child.stderr.take().unwrap());
    let mut line = String::new();
    stderr.read_line(&mut line).unwrap();
    assert!(line.starts_with("Watching"), "{line}");

    with_shot(&dir, "new.jxr");
    let made = dir.join("new_sdr.png");
    let start = Instant::now();
    while !made.exists() && start.elapsed() < Duration::from_secs(60) {
        std::thread::sleep(Duration::from_millis(200));
    }
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(made.exists(), "the new screenshot wasn't converted");
    // Images that were there before are left alone.
    assert!(!dir.join("old_sdr.png").exists());
}
