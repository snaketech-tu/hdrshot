//! Converts `fixtures/hdr64.jxr`, a small synthetic HDR capture, and checks
//! the results, so decoder updates or color changes can't slip through CI.
//! See `fixtures/make_hdr64.py` for how the file was made and what it shows.

use std::path::{Path, PathBuf};

use jxr2png_core::{
    ConvertOptions, InputOptions, Outputs, ScRgbImage, SdrOptions, convert_file, decode,
    for_discord, preview, sdr_rgb8,
};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/hdr64.jxr");

fn fixture() -> ScRgbImage {
    decode(&std::fs::read(FIXTURE).unwrap()).unwrap()
}

fn at(image: &ScRgbImage, x: usize, y: usize) -> [f32; 3] {
    image.pixels[y * image.width as usize + x]
}

/// A fresh output folder for one test.
fn out_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("jxr2png-fixture")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

struct Png {
    info: png::Info<'static>,
    pixels: Vec<u8>,
}

fn read_png(path: &Path) -> Png {
    read_png_bytes(&std::fs::read(path).unwrap())
}

fn read_png_bytes(bytes: &[u8]) -> Png {
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().unwrap();
    let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
    reader.next_frame(&mut pixels).unwrap();
    Png {
        info: reader.info().clone(),
        pixels,
    }
}

impl Png {
    fn rgb(&self, x: usize, y: usize) -> [u16; 3] {
        let i = (y * self.info.width as usize + x) * 3;
        match self.info.bit_depth {
            png::BitDepth::Sixteen => std::array::from_fn(|c| {
                u16::from_be_bytes([self.pixels[(i + c) * 2], self.pixels[(i + c) * 2 + 1]])
            }),
            _ => std::array::from_fn(|c| u16::from(self.pixels[i + c])),
        }
    }
}

fn convert(name: &str) -> Outputs {
    convert_file(
        Path::new(FIXTURE),
        &ConvertOptions {
            output_dir: Some(out_dir(name)),
            ..Default::default()
        },
    )
    .unwrap()
}

/// A pixel of the test image and what it should become.
struct Point {
    x: usize,
    y: usize,
    /// Linear scRGB in the JXR.
    source: [f32; 3],
    /// 16-bit PQ values in the HDR PNG.
    hdr: [u16; 3],
    /// 8-bit sRGB values in the SDR PNG.
    sdr: [u16; 3],
}

const fn point(x: usize, y: usize, source: [f32; 3], hdr: [u16; 3], sdr: [u16; 3]) -> Point {
    Point {
        x,
        y,
        source,
        hdr,
        sdr,
    }
}

const POINTS: [Point; 12] = [
    point(0, 0, [0.0, 0.0, 0.0], [0, 0, 0], [0, 0, 0]),
    point(
        63,
        0,
        [2.0, 2.0, 2.0],
        [36416, 36416, 36416],
        [250, 250, 250],
    ),
    point(4, 40, [1.0, 0.0, 0.0], [28864, 16704, 10752], [240, 0, 0]),
    point(12, 40, [0.0, 1.0, 0.0], [24960, 31296, 17856], [0, 240, 0]),
    point(20, 40, [0.0, 0.0, 1.0], [14592, 9536, 31104], [0, 0, 240]),
    point(
        28,
        40,
        [1.0, 1.0, 1.0],
        [31872, 31872, 31872],
        [240, 240, 240],
    ),
    point(
        36,
        40,
        [2.5, 2.5, 2.5],
        [37952, 37952, 37952],
        [251, 251, 251],
    ),
    point(
        44,
        40,
        [12.5, 12.5, 12.5],
        [49280, 49280, 49280],
        [255, 255, 255],
    ),
    point(52, 40, [1.2, -0.1, -0.05], [29696, 0, 0], [245, 0, 0]),
    point(60, 40, [0.0, 0.0, 0.0], [0, 0, 0], [0, 0, 0]),
    point(
        0,
        60,
        [1.0, 1.0, 1.0],
        [31872, 31872, 31872],
        [240, 240, 240],
    ),
    point(
        63,
        60,
        [25.0, 25.0, 25.0],
        [54208, 54208, 54208],
        [255, 255, 255],
    ),
];

#[test]
fn decodes_to_scrgb() {
    let image = fixture();
    assert_eq!((image.width, image.height), (64, 64));
    for Point { x, y, source, .. } in POINTS {
        let got = at(&image, x, y);
        for c in 0..3 {
            assert!(
                (got[c] - source[c]).abs() <= 1e-3 * source[c].abs().max(1.0),
                "({x}, {y}): {got:?}, expected {source:?}"
            );
        }
    }
}

#[test]
fn hdr_png_pixels_and_metadata() {
    let hdr = read_png(&convert("hdr").hdr.unwrap());
    assert_eq!(hdr.info.bit_depth, png::BitDepth::Sixteen);
    assert_eq!(hdr.info.color_type, png::ColorType::Rgb);
    for Point {
        x,
        y,
        hdr: expected,
        ..
    } in POINTS
    {
        assert_eq!(hdr.rgb(x, y), expected, "HDR pixel ({x}, {y})");
    }

    // Rec.2020 primaries, PQ transfer, full range: what Windows Photos and
    // HDR viewers need to show it in HDR.
    let cicp = hdr.info.coding_independent_code_points.unwrap();
    assert_eq!(
        (
            cicp.color_primaries,
            cicp.transfer_function,
            cicp.matrix_coefficients
        ),
        (9, 16, 0)
    );
    assert!(cicp.is_video_full_range_image);

    // MaxCLL is the 2000-nit highlight; MaxFALL the image's average.
    let clli = hdr.info.content_light_level.unwrap();
    assert_eq!(clli.max_content_light_level, 2000 * 10_000);
    assert_eq!(clli.max_frame_average_light_level, 3_479_008);
    let mdcv = hdr.info.mastering_display_color_volume.unwrap();
    assert_eq!(mdcv.max_luminance, 2000 * 10_000);

    // Discord drops ICC profiles of 32 KiB or more.
    let icc = hdr.info.icc_profile.unwrap();
    assert!(icc.len() < 32 * 1024, "ICC profile is {} bytes", icc.len());
}

#[test]
fn sdr_png_pixels() {
    let sdr = read_png(&convert("sdr").sdr.unwrap());
    assert_eq!(sdr.info.bit_depth, png::BitDepth::Eight);
    assert_eq!(sdr.info.srgb, Some(png::SrgbRenderingIntent::Perceptual));
    for Point {
        x,
        y,
        sdr: expected,
        ..
    } in POINTS
    {
        assert_eq!(sdr.rgb(x, y), expected, "SDR pixel ({x}, {y})");
    }
}

#[test]
fn discord_copy_drops_cicp_and_mdcv_only() {
    let path = convert("discord").hdr.unwrap();
    let copy = read_png_bytes(&for_discord(&std::fs::read(path).unwrap()));
    assert!(copy.info.coding_independent_code_points.is_none());
    assert!(copy.info.mastering_display_color_volume.is_none());
    assert!(copy.info.icc_profile.is_some());
    for Point {
        x,
        y,
        hdr: expected,
        ..
    } in POINTS
    {
        assert_eq!(copy.rgb(x, y), expected, "Discord copy pixel ({x}, {y})");
    }
}

#[test]
fn preview_is_small_and_tone_maps_like_the_png() {
    let preview = preview(Path::new(FIXTURE), &InputOptions::default(), 32, 16).unwrap();
    assert_eq!((preview.image.width, preview.image.height), (16, 16));
    // Levels come from the full image, not the downscaled one.
    let full = jxr2png_core::light_levels(&fixture());
    assert_eq!(preview.levels, full);

    let dir = out_dir("preview");
    let options = ConvertOptions {
        mode: jxr2png_core::OutputMode::Sdr,
        output_dir: Some(dir),
        ..Default::default()
    };
    let outputs = convert_file(Path::new(FIXTURE), &options).unwrap();
    let png = read_png(outputs.sdr.as_ref().unwrap());
    let full_rgb = sdr_rgb8(&fixture(), &full, &SdrOptions::default());
    assert_eq!(png.pixels, full_rgb);
}
