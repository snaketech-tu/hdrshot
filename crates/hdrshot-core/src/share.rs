//! Copies of the PNGs for pasting elsewhere, chiefly into Discord.

use std::path::{Path, PathBuf};

use crate::{ConvertOptions, Result, input, light_levels, resize, write_hdr_png, write_sdr_png};

/// Largest file Discord takes from an account without Nitro.
pub const DISCORD_LIMIT: u64 = 10_000_000;

/// Heights tried, largest first, when a copy has to be made smaller.
const SMALLER_HEIGHTS: [u32; 3] = [1440, 1080, 720];

/// A PNG made smaller to fit a size limit.
pub struct Smaller {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Writes `png` to a temporary folder that only ever holds the latest copy,
/// as `name`, and returns its path. An HDR PNG loses its `cICP` chunk
/// first (see [`crate::for_discord`]).
pub fn clipboard_file(name: &str, png: &[u8], hdr: bool) -> std::io::Result<PathBuf> {
    let dir = std::env::temp_dir().join("hdrshot-clipboard");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(name);
    if hdr {
        std::fs::write(&path, crate::for_discord(png))?;
    } else {
        std::fs::write(&path, png)?;
    }
    Ok(path)
}

/// The HDR (or, with `hdr` false, SDR) PNG of `input`, scaled down to the
/// largest of 1440p, 1080p and 720p, below the height it would have with
/// `options`, at which it fits in `limit` bytes. HDR copies are measured
/// without their `cICP` chunk, as [`clipboard_file`] writes them. `None`
/// when even 720p is too large, or the image is no taller than that.
pub fn smaller_png(
    input: &Path,
    options: &ConvertOptions,
    hdr: bool,
    limit: u64,
) -> Result<Option<Smaller>> {
    smaller_png_at(input, options, hdr, limit, &SMALLER_HEIGHTS)
}

fn smaller_png_at(
    input: &Path,
    options: &ConvertOptions,
    hdr: bool,
    limit: u64,
    heights: &[u32],
) -> Result<Option<Smaller>> {
    let image = input::decode_file(input, &options.input)?;
    let height = options
        .max_height
        .map_or(image.height, |max| image.height.min(max));
    for &max_height in heights.iter().filter(|&&h| h < height) {
        let (width, height) = resize::fit_height(image.width, image.height, max_height);
        let small = resize::resize(&image, width, height);
        let levels = light_levels(&small);
        let mut png = Vec::new();
        if hdr {
            write_hdr_png(&small, &levels, &options.hdr, &options.sdr, &mut png)?;
            png = crate::for_discord(&png);
        } else {
            write_sdr_png(&small, &levels, &options.sdr, &mut png)?;
        }
        if png.len() as u64 <= limit {
            return Ok(Some(Smaller { png, width, height }));
        }
    }
    Ok(None)
}

/// File size in megabytes, as Discord counts them, e.g. "14.2 MB".
pub fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_000_000.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode;

    const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/hdr64.jxr");

    #[test]
    fn smaller_png_steps_down_until_it_fits() {
        let options = ConvertOptions::default();
        let fixture = Path::new(FIXTURE);
        let try_limit = |limit, hdr| {
            smaller_png_at(fixture, &options, hdr, limit, &[64, 48, 32, 16])
                .unwrap()
                .map(|s| (s.width, s.height, s.png.len() as u64))
        };
        // 64 is not below the fixture's height, so 48 is the first step.
        let (w, h, size) = try_limit(u64::MAX, true).unwrap();
        assert_eq!((w, h), (48, 48));
        // Just under that size, it has to go a step further.
        let (w, h, smaller) = try_limit(size - 1, true).unwrap();
        assert!(w < 48 && h < 48 && smaller < size);
        assert!(try_limit(1, false).is_none());

        let (_, _, png) = smaller_png_at(fixture, &options, true, u64::MAX, &[32])
            .unwrap()
            .map(|s| (s.width, s.height, s.png))
            .unwrap();
        assert!(!png.windows(4).any(|w| w == b"cICP"));
    }

    #[test]
    fn clipboard_file_strips_cicp_from_hdr_only() {
        let mut png = Vec::new();
        let image = decode(&std::fs::read(FIXTURE).unwrap()).unwrap();
        let options = ConvertOptions::default();
        write_hdr_png(
            &image,
            &light_levels(&image),
            &options.hdr,
            &options.sdr,
            &mut png,
        )
        .unwrap();
        assert!(png.windows(4).any(|w| w == b"cICP"));
        let hdr = std::fs::read(clipboard_file("a.png", &png, true).unwrap()).unwrap();
        assert!(!hdr.windows(4).any(|w| w == b"cICP"));
        let kept = std::fs::read(clipboard_file("b.png", &png, false).unwrap()).unwrap();
        assert_eq!(kept, png);
    }

    #[test]
    fn megabytes_are_decimal() {
        assert_eq!(megabytes(14_230_000), "14.2 MB");
        assert_eq!(megabytes(DISCORD_LIMIT), "10.0 MB");
    }
}
