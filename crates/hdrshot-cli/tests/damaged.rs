//! A damaged image fails on its own, without taking the rest of the batch
//! (or the program) down with it.

use std::path::{Path, PathBuf};
use std::process::Command;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("hdrshot-cli")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn damaged_avif_fails_alone() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../hdrshot-core/tests/fixtures");
    let dir = temp_dir("damaged");
    // One changed byte in the AV1 data makes rav1d panic inside its C
    // interface, which would abort the program.
    let mut bytes = std::fs::read(fixtures.join("pq_p3.avif")).unwrap();
    bytes[4645] = 0xFF;
    std::fs::write(dir.join("damaged.avif"), bytes).unwrap();
    std::fs::copy(fixtures.join("pq_p3.avif"), dir.join("good.avif")).unwrap();
    std::fs::copy(fixtures.join("hdr64.jxr"), dir.join("shot.jxr")).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_hdrshot"))
        .arg("--sdr")
        .args(["damaged.avif", "good.avif", "shot.jxr"].map(|f| dir.join(f)))
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(
        stderr.contains("damaged.avif: could not decode image"),
        "{stderr}"
    );
    assert!(stderr.contains("1 of 3 files failed"), "{stderr}");
    assert!(dir.join("good_sdr.png").exists());
    assert!(dir.join("shot_sdr.png").exists());
    assert!(!dir.join("damaged_sdr.png").exists());
}
