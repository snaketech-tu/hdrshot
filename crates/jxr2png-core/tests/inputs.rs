//! Inputs other than opaque HDR captures: an HDR image with transparency and
//! an 8-bit SDR image. See `fixtures/make_inputs.py`.

use std::path::{Path, PathBuf};

use jxr2png_core::{ConvertOptions, OutputMode, convert_file, decode, resize};

const ALPHA: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/alpha32.jxr");
const SDR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/sdr32.jxr");

fn out_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("jxr2png-inputs")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn read_png(path: &Path) -> (png::ColorType, png::BitDepth, Vec<u8>) {
    let decoder = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).unwrap()));
    let mut reader = decoder.read_info().unwrap();
    let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut pixels).unwrap();
    pixels.truncate(info.buffer_size());
    (info.color_type, info.bit_depth, pixels)
}

#[test]
fn alpha_is_decoded() {
    let image = decode(&std::fs::read(ALPHA).unwrap()).unwrap();
    assert!(image.hdr);
    let alpha = image.alpha.as_ref().expect("alpha kept");
    assert!(
        (alpha[0] - 1.0).abs() < 0.01,
        "left edge opaque: {}",
        alpha[0]
    );
    assert!(alpha[31] < 0.01, "right edge transparent: {}", alpha[31]);
}

#[test]
fn alpha_reaches_both_pngs() {
    let options = ConvertOptions {
        output_dir: Some(out_dir("alpha")),
        ..Default::default()
    };
    let outputs = convert_file(Path::new(ALPHA), &options).unwrap();

    let (color, depth, hdr) = read_png(outputs.hdr.as_ref().unwrap());
    assert_eq!(
        (color, depth),
        (png::ColorType::Rgba, png::BitDepth::Sixteen)
    );
    let alpha_at = |x: usize| u16::from_be_bytes([hdr[x * 8 + 6], hdr[x * 8 + 7]]);
    assert!(alpha_at(0) > 65_000 && alpha_at(31) < 500);

    let (color, depth, sdr) = read_png(outputs.sdr.as_ref().unwrap());
    assert_eq!((color, depth), (png::ColorType::Rgba, png::BitDepth::Eight));
    assert!(sdr[3] > 250 && sdr[31 * 4 + 3] < 3);
    assert!(!outputs.sdr_input);
}

#[test]
fn transparent_pixels_do_not_darken_downscaling() {
    let image = decode(&std::fs::read(ALPHA).unwrap()).unwrap();
    let small = resize::resize(&image, 8, 8);
    let alpha = small.alpha.as_ref().unwrap();
    for (p, &a) in small.pixels.iter().zip(alpha) {
        if a > 0.05 {
            for c in p {
                assert!((c - 2.0).abs() < 0.05, "color {c} at alpha {a}");
            }
        }
    }
}

#[test]
fn sdr_input_makes_only_the_sdr_png() {
    let image = decode(&std::fs::read(SDR).unwrap()).unwrap();
    assert!(!image.hdr);
    assert!(image.alpha.is_none());

    let options = ConvertOptions {
        output_dir: Some(out_dir("sdr")),
        ..Default::default()
    };
    let outputs = convert_file(Path::new(SDR), &options).unwrap();
    assert!(outputs.sdr_input);
    assert_eq!(outputs.hdr, None);

    // The SDR PNG is the input picture, unchanged.
    let (color, _, pixels) = read_png(outputs.sdr.as_ref().unwrap());
    assert_eq!(color, png::ColorType::Rgb);
    for y in 0..32usize {
        for x in 0..32usize {
            let at = (y * 32 + x) * 3;
            let want = [255 - x * 8, y * 8, x * 8];
            for c in 0..3 {
                let got = usize::from(pixels[at + c]);
                assert!(
                    got.abs_diff(want[c]) <= 1,
                    "({x}, {y}) {c}: {got} vs {}",
                    want[c]
                );
            }
        }
    }
}

#[test]
fn sdr_input_with_hdr_only_still_makes_the_sdr_png() {
    let options = ConvertOptions {
        mode: OutputMode::Hdr,
        output_dir: Some(out_dir("sdr-hdr-only")),
        ..Default::default()
    };
    let outputs = convert_file(Path::new(SDR), &options).unwrap();
    assert!(outputs.sdr_input);
    assert_eq!(outputs.hdr, None);
    assert!(outputs.sdr.as_ref().unwrap().ends_with("sdr32_sdr.png"));
}
