//! Downscaling in linear light.
//!
//! scRGB is linear, so averaging pixels here gives the physically right
//! result (a sRGB- or PQ-encoded image would darken edges). The filter is a
//! triangle as wide as the scale factor: unlike Lanczos it never rings, which
//! matters in HDR, where a 1000-nit highlight next to a dark pixel would
//! otherwise leave bright and negative halos.

use rayon::prelude::*;

use crate::decode::ScRgbImage;

/// Output size for an image `width`×`height` limited to `max_height`,
/// keeping the aspect ratio. Never scales up.
pub fn fit_height(width: u32, height: u32, max_height: u32) -> (u32, u32) {
    if max_height == 0 || height <= max_height {
        return (width, height);
    }
    let new_width = (f64::from(width) * f64::from(max_height) / f64::from(height)).round();
    ((new_width as u32).max(1), max_height)
}

/// Output size for an image `width`×`height` that fits in
/// `max_width`×`max_height`, keeping the aspect ratio. Never scales up.
pub fn fit_box(width: u32, height: u32, max_width: u32, max_height: u32) -> (u32, u32) {
    if width <= max_width && height <= max_height {
        return (width, height);
    }
    let scale =
        (f64::from(max_width) / f64::from(width)).min(f64::from(max_height) / f64::from(height));
    let scaled = |v: u32| ((f64::from(v) * scale).round() as u32).max(1);
    (scaled(width), scaled(height))
}

/// Resample `image` to `width`×`height`. Colors are weighted by alpha, so
/// transparent pixels don't bleed into their neighbours.
pub fn resize(image: &ScRgbImage, width: u32, height: u32) -> ScRgbImage {
    if (width, height) == (image.width, image.height) {
        return image.clone();
    }
    let src = (image.width as usize, image.height as usize);
    let dst = (width as usize, height as usize);
    let (pixels, alpha) = match &image.alpha {
        None => (resample(&image.pixels, src, dst), None),
        Some(alpha) => {
            let premultiplied: Vec<[f32; 3]> = image
                .pixels
                .par_iter()
                .zip(alpha.par_iter())
                .map(|(p, &a)| p.map(|c| c * a))
                .collect();
            let as_rgb: Vec<[f32; 3]> = alpha.iter().map(|&a| [a; 3]).collect();
            let alpha: Vec<f32> = resample(&as_rgb, src, dst)
                .into_iter()
                .map(|a| a[0].clamp(0.0, 1.0))
                .collect();
            let pixels = resample(&premultiplied, src, dst)
                .into_par_iter()
                .zip(alpha.par_iter())
                .map(|(p, &a)| if a > 0.0 { p.map(|c| c / a) } else { [0.0; 3] })
                .collect();
            (pixels, Some(alpha))
        }
    };
    ScRgbImage {
        width,
        height,
        pixels,
        alpha,
        hdr: image.hdr,
        sdr_white: image.sdr_white,
    }
}

/// Resamples `pixels`, `src` (width, height), to `dst`.
fn resample(pixels: &[[f32; 3]], src: (usize, usize), dst: (usize, usize)) -> Vec<[f32; 3]> {
    let ((src_w, src_h), (dst_w, dst_h)) = (src, dst);

    // Horizontal pass: src_w × src_h -> dst_w × src_h.
    let columns = weights(src_w, dst_w);
    let mut wide = vec![[0.0f32; 3]; dst_w * src_h];
    wide.par_chunks_mut(dst_w)
        .zip(pixels.par_chunks(src_w))
        .for_each(|(out, row)| {
            for (o, (start, w)) in out.iter_mut().zip(&columns) {
                *o = blend(w.iter().enumerate().map(|(k, &w)| (row[start + k], w)));
            }
        });

    // Vertical pass: dst_w × src_h -> dst_w × dst_h.
    let rows = weights(src_h, dst_h);
    let mut out = vec![[0.0f32; 3]; dst_w * dst_h];
    out.par_chunks_mut(dst_w)
        .zip(rows.par_iter())
        .for_each(|(out, (start, w))| {
            for (x, o) in out.iter_mut().enumerate() {
                *o = blend(
                    w.iter()
                        .enumerate()
                        .map(|(k, &w)| (wide[(start + k) * dst_w + x], w)),
                );
            }
        });
    out
}

fn blend(taps: impl Iterator<Item = ([f32; 3], f32)>) -> [f32; 3] {
    let mut sum = [0.0f32; 3];
    for (p, w) in taps {
        for c in 0..3 {
            sum[c] += p[c] * w;
        }
    }
    sum
}

/// For each output sample, the first source sample it reads and the
/// normalized weights of the samples from there on.
fn weights(src: usize, dst: usize) -> Vec<(usize, Vec<f32>)> {
    let scale = src as f64 / dst as f64;
    let radius = scale.max(1.0);
    (0..dst)
        .map(|i| {
            let center = (i as f64 + 0.5) * scale - 0.5;
            let first = ((center - radius).floor() + 1.0).max(0.0) as usize;
            let last = ((center + radius).ceil() - 1.0).min(src as f64 - 1.0) as usize;
            let mut w: Vec<f64> = (first..=last)
                .map(|j| (1.0 - (j as f64 - center).abs() / radius).max(0.0))
                .collect();
            let total: f64 = w.iter().sum();
            if total > 0.0 {
                w.iter_mut().for_each(|v| *v /= total);
            }
            (first, w.into_iter().map(|v| v as f32).collect())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_box_keeps_aspect_and_never_upscales() {
        assert_eq!(fit_box(3440, 1440, 480, 270), (480, 201));
        assert_eq!(fit_box(1440, 3440, 480, 270), (113, 270));
        assert_eq!(fit_box(64, 64, 480, 270), (64, 64));
        assert_eq!(fit_box(10_000, 1, 96, 54), (96, 1));
    }

    #[test]
    fn fit_height_keeps_aspect_and_never_upscales() {
        assert_eq!(fit_height(3440, 1440, 1080), (2580, 1080));
        assert_eq!(fit_height(3840, 2160, 1440), (2560, 1440));
        assert_eq!(fit_height(1920, 1080, 1440), (1920, 1080));
        assert_eq!(fit_height(1920, 1080, 0), (1920, 1080));
    }

    #[test]
    fn halving_averages_and_keeps_flat_areas() {
        // A flat 80-nit area stays exactly flat.
        let flat = ScRgbImage::new(8, 6, vec![[1.0; 3]; 48]);
        let out = resize(&flat, 4, 3);
        assert_eq!((out.width, out.height), (4, 3));
        assert!(out.pixels.iter().flatten().all(|&c| (c - 1.0).abs() < 1e-6));

        // A fine checker of 0 and 10 blurs to about its average, 5.
        let checker = ScRgbImage::new(
            8,
            8,
            (0..64)
                .map(|i| [if (i % 8 + i / 8) % 2 == 0 { 0.0 } else { 10.0 }; 3])
                .collect(),
        );
        let out = resize(&checker, 4, 4);
        for &c in out.pixels.iter().flatten() {
            assert!((3.0..=7.0).contains(&c), "{c}");
        }
    }

    #[test]
    fn no_ringing_next_to_highlights() {
        // A bright spike in a dark row: every output stays within the input
        // range, so no negative or brighter-than-source halos.
        let mut pixels = vec![[0.01f32; 3]; 30];
        pixels[15] = [12.5; 3];
        let image = ScRgbImage::new(30, 1, pixels);
        let out = resize(&image, 11, 1);
        for p in &out.pixels {
            for &c in p {
                assert!((0.01 - 1e-6..=12.5).contains(&c), "{c}");
            }
        }
    }
}
