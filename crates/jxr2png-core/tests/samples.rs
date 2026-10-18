//! Converts real `.jxr` files when `JXR2PNG_SAMPLES` points at a folder of them.
//! Sample captures are large, so they aren't kept in the repository.

use jxr2png_core::{ConvertOptions, convert_file};

#[test]
fn convert_sample_folder() {
    let Some(dir) = std::env::var_os("JXR2PNG_SAMPLES") else {
        eprintln!("JXR2PNG_SAMPLES not set; skipping");
        return;
    };
    let out = std::env::temp_dir().join("jxr2png-samples");
    let options = ConvertOptions {
        output_dir: Some(out),
        ..Default::default()
    };
    let mut converted = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("jxr"))
        {
            let outputs = convert_file(&path, &options).unwrap();
            assert!(outputs.hdr.unwrap().exists());
            assert!(outputs.sdr.unwrap().exists());
            converted += 1;
        }
    }
    assert!(converted > 0, "no .jxr files found");
}
