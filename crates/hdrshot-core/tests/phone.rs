//! Phone HDR photos: gain map JPEGs and HEICs. See `fixtures/SOURCES.md`.
//!
//! The expected brightness comes from an independent reference: the files
//! decoded with Pillow and libheif, the gain maps applied by Apple's and
//! Adobe's published formulas, SDR white at 203 nits.

use std::path::{Path, PathBuf};

use hdrshot_core::{
    ConvertOptions, HdrFormat, HdrOptions, InputFormat, InputOptions, OutputMode, ScRgbImage,
    SdrOptions, convert_bytes, find_inputs, input,
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

fn assert_close(got: f32, want: f32, what: &str) {
    assert!(
        (got - want).abs() <= want * 0.02,
        "{what}: {got} nits, expected {want}"
    );
}

#[track_caller]
fn check(name: &str, size: (u32, u32), mean: f32, peak: f32) {
    let image = decode(name);
    assert!(image.hdr);
    assert_eq!((image.width, image.height), size);
    assert_eq!(image.sdr_white, Some(203.0));
    assert_eq!(input::dimensions(&fixture(name)).unwrap(), size);
    let (got_mean, got_peak) = luminance(&image);
    assert_close(got_mean, mean, "mean");
    assert_close(got_peak, peak, "peak");
}

#[test]
fn iphone_jpeg_with_headroom_in_exif() {
    check("apple_gainmap_old.jpg", (384, 512), 49.8, 861.0);
}

#[test]
fn iphone_jpeg_with_headroom_in_xmp() {
    check("apple_gainmap_new.jpg", (384, 512), 42.6, 531.0);
}

#[test]
fn ultra_hdr_jpeg() {
    check("paris_gainmap.jpg", (403, 302), 186.3, 1118.0);
}

#[test]
fn iphone_heic() {
    check("iphone_gainmap.heic", (1512, 850), 105.1, 1496.0);
}

#[test]
fn formats_are_recognized() {
    let sniff = |name: &str| InputFormat::sniff(&std::fs::read(fixture(name)).unwrap());
    assert_eq!(sniff("paris_gainmap.jpg"), Some(InputFormat::Jpeg));
    assert_eq!(sniff("iphone_gainmap.heic"), Some(InputFormat::Heif));
}

#[test]
fn input_white_sets_sdr_white() {
    let options = InputOptions {
        white_nits: Some(100.0),
    };
    let image = input::decode_file(&fixture("paris_gainmap.jpg"), &options).unwrap();
    assert_eq!(image.sdr_white, Some(100.0));
    let (mean, _) = luminance(&image);
    assert_close(mean, 186.3 * 100.0 / 203.0, "mean");
}

#[test]
fn sdr_output_keeps_the_photo_white() {
    // The SDR output's white is the photo's, 203 nits, not the 80 nits of
    // screenshots, so it is about as bright as the photo's own SDR picture.
    let bytes = std::fs::read(fixture("apple_gainmap_new.jpg")).unwrap();
    let options = ConvertOptions {
        mode: OutputMode::Sdr,
        ..Default::default()
    };
    let sdr = convert_bytes(&bytes, &options).unwrap().sdr.unwrap();
    let mean_png = mean_png_value(&sdr);
    let base = zune_mean(&bytes);
    assert!(
        (mean_png - base).abs() < base * 0.08,
        "SDR PNG mean {mean_png} vs the photo's SDR picture {base}"
    );
    let with = SdrOptions::default().for_image(&decode("apple_gainmap_new.jpg"));
    assert_eq!(with.white_nits, 203.0);
}

#[test]
fn ultra_hdr_from_hdrshot_reads_back() {
    // An Ultra HDR JPEG made by hdrshot reads back as the image it came from,
    // at hdrshot's own 80-nit SDR white.
    let pixels = (0..64 * 32)
        .map(|i| {
            let v = (i % 64) as f32 / 63.0 * 12.5;
            [v, v * 0.8, v * 0.6]
        })
        .collect();
    let source = ScRgbImage::new(64, 32, pixels);
    let options = ConvertOptions {
        mode: OutputMode::Hdr,
        hdr: HdrOptions {
            format: HdrFormat::UltraHdr,
            ..Default::default()
        },
        ..Default::default()
    };
    let jpeg = hdrshot_core::convert_image(source.clone(), &options)
        .unwrap()
        .hdr
        .unwrap();
    let back = input::decode(&jpeg, &InputOptions::default()).unwrap();
    assert_eq!(back.sdr_white, Some(80.0));
    let (want, _) = luminance(&source);
    let (got, _) = luminance(&back);
    assert_close(got, want, "mean");
}

#[test]
fn folder_scan_takes_phone_photos_but_not_plain_or_own_jpegs() {
    let dir = std::env::temp_dir().join(format!("hdrshot-phone-scan-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for name in ["paris_gainmap.jpg", "iphone_gainmap.heic"] {
        std::fs::copy(fixture(name), dir.join(name)).unwrap();
    }
    // A plain JPEG, and an Ultra HDR JPEG written by hdrshot.
    let plain = {
        let mut out = Vec::new();
        let image = ScRgbImage::new(2, 2, vec![[0.5; 3]; 4]);
        hdrshot_core::write_sdr_jpeg(
            &image,
            &hdrshot_core::light_levels(&image),
            &SdrOptions::default(),
            90,
            &mut out,
        )
        .unwrap();
        out
    };
    std::fs::write(dir.join("plain.jpg"), plain).unwrap();
    let ours = {
        let image = ScRgbImage::new(2, 2, vec![[5.0; 3]; 4]);
        let mut out = Vec::new();
        hdrshot_core::write_ultra_hdr(
            &image,
            &hdrshot_core::light_levels(&image),
            &HdrOptions::default(),
            &SdrOptions::default(),
            &mut out,
        )
        .unwrap();
        out
    };
    std::fs::write(dir.join("shot_hdr.jpg"), ours).unwrap();
    let names: Vec<_> = find_inputs(&dir, false)
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["iphone_gainmap.heic", "paris_gainmap.jpg"]);
}

/// Mean 8-bit value of an RGB PNG.
fn mean_png_value(bytes: &[u8]) -> f32 {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    buf.truncate(info.buffer_size());
    buf.iter().map(|&v| f32::from(v)).sum::<f32>() / buf.len() as f32
}

/// Mean 8-bit value of the first image of a JPEG.
fn zune_mean(bytes: &[u8]) -> f32 {
    let mut d = jpeg_decoder::Decoder::new(bytes);
    let pixels = d.decode().unwrap();
    pixels.iter().map(|&v| f32::from(v)).sum::<f32>() / pixels.len() as f32
}

#[test]
fn exif_orientation_turns_the_photo_upright() {
    let bytes = std::fs::read(fixture("paris_gainmap.jpg")).unwrap();
    let upright = input::decode(&bytes, &InputOptions::default()).unwrap();
    // The little-endian orientation entry: tag 0x0112, SHORT, count 1,
    // value 1. Make it 6, "turn clockwise".
    let entry = b"\x12\x01\x03\x00\x01\x00\x00\x00\x01\x00";
    let at = bytes.windows(entry.len()).position(|w| w == entry).unwrap();
    let mut turned = bytes.clone();
    turned[at + 8] = 6;
    let image = input::decode(&turned, &InputOptions::default()).unwrap();
    let (w, h) = (upright.width as usize, upright.height as usize);
    assert_eq!((image.width as usize, image.height as usize), (h, w));
    // The stored picture's bottom-left pixel ends up top-left.
    assert_eq!(image.pixels[0], upright.pixels[(h - 1) * w]);
    assert_eq!(image.pixels[h - 1], upright.pixels[0]);
}
