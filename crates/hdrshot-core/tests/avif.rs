//! AVIF input: PQ AVIFs, AVIFs with a gain map, and hdrshot's own AVIFs.
//! See `fixtures/SOURCES.md`.
//!
//! The expected brightness comes from an independent reference: the files
//! decoded with libavif (through Pillow) and libdav1d (through FFmpeg), the
//! gain map applied by the ISO 21496-1 formula, SDR white at 203 nits.

use std::path::{Path, PathBuf};

use hdrshot_core::{
    AvifOptions, ConvertOptions, HdrFormat, HdrOptions, InputFormat, InputOptions, OutputMode,
    ScRgbImage, find_inputs, input,
};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn decode(name: &str) -> ScRgbImage {
    input::decode_file(&fixture(name), &InputOptions::default()).unwrap()
}

/// Mean and largest luminance in nits.
fn luminance(image: &ScRgbImage) -> (f32, f32) {
    let nits: Vec<f32> = image
        .pixels
        .iter()
        .map(|p| (0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2]) * 80.0)
        .collect();
    let mean = nits.iter().sum::<f32>() / nits.len() as f32;
    (mean, nits.into_iter().fold(0.0, f32::max))
}

#[track_caller]
fn assert_close(got: f32, want: f32, what: &str) {
    assert!(
        (got - want).abs() <= want * 0.02,
        "{what}: {got} nits, expected {want}"
    );
}

#[track_caller]
fn check(name: &str, size: (u32, u32), mean: f32, peak: f32) -> ScRgbImage {
    let image = decode(name);
    assert!(image.hdr);
    assert_eq!((image.width, image.height), size);
    assert_eq!(input::dimensions(&fixture(name)).unwrap(), size);
    let (got_mean, got_peak) = luminance(&image);
    assert_close(got_mean, mean, "mean");
    assert_close(got_peak, peak, "peak");
    image
}

#[test]
fn pq_avif() {
    let image = check("pq_p3.avif", (200, 200), 261.8, 475.0);
    assert_eq!(image.sdr_white, None);
}

#[test]
fn sdr_avif_with_gain_map() {
    let image = check("seine_gainmap.avif", (400, 300), 164.8, 487.0);
    assert_eq!(image.sdr_white, Some(203.0));
}

#[test]
fn grid_with_alpha() {
    // A 4x3 grid of color tiles, a 4x3 grid of alpha tiles, and a gain map
    // to SDR, which the PQ picture doesn't need.
    let image = check("grid_alpha.avif", (512, 600), 773.5, 2916.0);
    let alpha = image.alpha.expect("alpha");
    let mean = alpha.iter().sum::<f32>() / alpha.len() as f32;
    assert!((mean - 0.4994).abs() < 0.005, "mean alpha {mean}");
}

#[test]
fn avif_is_recognized() {
    let bytes = std::fs::read(fixture("pq_p3.avif")).unwrap();
    assert_eq!(InputFormat::sniff(&bytes), Some(InputFormat::Avif));
    assert_eq!(
        InputFormat::from_path(Path::new("photo.AVIF")),
        Some(InputFormat::Avif)
    );
}

#[test]
fn rotation_turns_the_picture() {
    let bytes = std::fs::read(fixture("pq_p3.avif")).unwrap();
    let upright = input::decode(&bytes, &InputOptions::default()).unwrap();
    // Replace the 12-byte `clli` property with an `irot` of the same size:
    // a quarter turn anticlockwise.
    let clli = b"\0\0\0\x0cclli";
    let at = bytes.windows(clli.len()).position(|w| w == clli).unwrap();
    let mut turned = bytes.clone();
    turned[at..at + 12].copy_from_slice(b"\0\0\0\x0cirot\x01\0\0\0");
    let image = input::decode(&turned, &InputOptions::default()).unwrap();
    let w = upright.width as usize;
    assert_eq!((image.width, image.height), (upright.height, upright.width));
    // The stored picture's top-right pixel ends up top-left.
    assert_eq!(image.pixels[0], upright.pixels[w - 1]);
}

/// An image with a gradient from black to 1000 nits, and transparency.
fn source() -> ScRgbImage {
    let (w, h) = (64, 32);
    let pixels = (0..w * h)
        .map(|i| {
            let v = (i % w) as f32 / (w - 1) as f32 * 12.5;
            [v, v * 0.8, v * 0.6]
        })
        .collect();
    let mut image = ScRgbImage::new(w as u32, h as u32, pixels);
    image.alpha = Some((0..w * h).map(|i| if i < w { 0.5 } else { 1.0 }).collect());
    image
}

fn avif_of(image: &ScRgbImage) -> Vec<u8> {
    let options = ConvertOptions {
        mode: OutputMode::Hdr,
        hdr: HdrOptions {
            format: HdrFormat::Avif,
            avif: AvifOptions {
                quality: 100,
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    hdrshot_core::convert_image(image.clone(), &options)
        .unwrap()
        .hdr
        .unwrap()
}

#[test]
fn avif_from_hdrshot_reads_back() {
    let source = source();
    let back = input::decode(&avif_of(&source), &InputOptions::default()).unwrap();
    assert_eq!((back.width, back.height), (source.width, source.height));
    let (want, want_peak) = luminance(&source);
    let (got, got_peak) = luminance(&back);
    assert_close(got, want, "mean");
    assert_close(got_peak, want_peak, "peak");
    let alpha = back.alpha.expect("alpha");
    assert!((alpha[0] - 0.5).abs() < 0.01 && alpha[64 * 31] > 0.99);
}

#[test]
fn folder_scan_takes_gain_map_avifs_but_not_own_avifs() {
    let dir = std::env::temp_dir().join(format!("hdrshot-avif-scan-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(fixture("seine_gainmap.avif"), dir.join("photo.avif")).unwrap();
    std::fs::write(dir.join("shot_hdr.avif"), avif_of(&source())).unwrap();
    let names: Vec<_> = find_inputs(&dir, false)
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(names, ["photo.avif"]);
}
