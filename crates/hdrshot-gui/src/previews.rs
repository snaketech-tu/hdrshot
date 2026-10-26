//! Small previews of the listed images, decoded one at a time on a
//! background thread so adding many files doesn't stall the window.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};

use half::f16;
use hdrshot_core::{LightLevels, Preview, ScRgbImage, SdrOptions};

/// Largest preview kept per file. The tooltip shows it at this size.
pub const PREVIEW_SIZE: (u32, u32) = (360, 220);
/// Size of a thumbnail in the file list, which crops the image to fill it.
pub const THUMB_SIZE: (u32, u32) = (80, 45);

pub enum Thumb {
    Loading,
    Ready {
        preview: Box<Stored>,
        texture: egui::TextureHandle,
    },
    Failed,
}

pub struct Previews {
    requests: Sender<PathBuf>,
    results: Receiver<(PathBuf, Result<Preview, String>)>,
}

impl Previews {
    pub fn new(ctx: egui::Context) -> Self {
        let (requests, queue) = channel::<PathBuf>();
        let (done, results) = channel();
        std::thread::spawn(move || {
            for path in queue {
                let (w, h) = PREVIEW_SIZE;
                let preview = hdrshot_core::preview(&path, &Default::default(), w, h)
                    .map_err(|e| e.to_string());
                if done.send((path, preview)).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        });
        Self { requests, results }
    }

    pub fn request(&self, path: &Path) {
        let _ = self.requests.send(path.to_path_buf());
    }

    /// Previews finished since the last call, as thumbnails ready to show.
    pub fn finished(&self, ctx: &egui::Context) -> Vec<(PathBuf, Thumb)> {
        self.results
            .try_iter()
            .map(|(path, preview)| {
                let thumb = match preview {
                    Ok(preview) => {
                        let (tw, th) = cover_size(preview.image.width, preview.image.height);
                        let small = hdrshot_core::resize::resize(&preview.image, tw, th);
                        let texture = texture(
                            ctx,
                            "thumb",
                            &Preview {
                                image: small,
                                levels: preview.levels,
                            },
                            &SdrOptions::default(),
                        );
                        Thumb::Ready {
                            preview: Box::new(Stored::new(&preview)),
                            texture,
                        }
                    }
                    Err(_) => Thumb::Failed,
                };
                (path, thumb)
            })
            .collect()
    }
}

/// Size of a `width` × `height` image scaled to cover twice
/// [`THUMB_SIZE`], for sharp thumbnails on high-DPI screens, but never
/// scaled up.
fn cover_size(width: u32, height: u32) -> (u32, u32) {
    let (w, h) = THUMB_SIZE;
    let scale = f64::max(
        f64::from(2 * w) / f64::from(width.max(1)),
        f64::from(2 * h) / f64::from(height.max(1)),
    )
    .min(1.0);
    let scaled = |n: u32| ((f64::from(n) * scale).round() as u32).max(1);
    (scaled(width), scaled(height))
}

/// A preview as kept for its row: half floats and 8-bit alpha take less
/// than half the memory of the float preview, with no visible difference
/// once tone-mapped to 8 bits.
pub struct Stored {
    width: u32,
    height: u32,
    pixels: Vec<[f16; 3]>,
    alpha: Option<Vec<u8>>,
    hdr: bool,
    sdr_white: Option<f32>,
    levels: LightLevels,
}

impl Stored {
    fn new(preview: &Preview) -> Self {
        let image = &preview.image;
        Self {
            width: image.width,
            height: image.height,
            pixels: image
                .pixels
                .iter()
                .map(|p| p.map(|c| f16::from_f32(c.min(f16::MAX.to_f32()))))
                .collect(),
            alpha: image
                .alpha
                .as_ref()
                .map(|alpha| alpha.iter().map(|&a| (a * 255.0).round() as u8).collect()),
            hdr: image.hdr,
            sdr_white: image.sdr_white,
            levels: preview.levels,
        }
    }

    /// The preview in floats again, for tone-mapping it.
    pub fn restore(&self) -> Preview {
        Preview {
            image: ScRgbImage {
                width: self.width,
                height: self.height,
                pixels: self.pixels.iter().map(|p| p.map(f16::to_f32)).collect(),
                alpha: self
                    .alpha
                    .as_ref()
                    .map(|alpha| alpha.iter().map(|&a| f32::from(a) / 255.0).collect()),
                hdr: self.hdr,
                sdr_white: self.sdr_white,
            },
            levels: self.levels,
        }
    }
}

/// `preview` tone-mapped like the SDR PNG, as a texture.
pub fn texture(
    ctx: &egui::Context,
    name: &str,
    preview: &Preview,
    sdr: &SdrOptions,
) -> egui::TextureHandle {
    ctx.load_texture(name, sdr_image(preview, sdr), egui::TextureOptions::LINEAR)
}

/// `preview` tone-mapped like the SDR PNG, as an image for a texture.
pub fn sdr_image(preview: &Preview, sdr: &SdrOptions) -> egui::ColorImage {
    let image = &preview.image;
    let rgb = hdrshot_core::sdr_rgb8(image, &preview.levels, sdr);
    let size = [image.width as usize, image.height as usize];
    match &image.alpha {
        None => egui::ColorImage::from_rgb(size, &rgb),
        Some(alpha) => {
            let rgba: Vec<u8> = rgb
                .as_chunks::<3>()
                .0
                .iter()
                .zip(alpha)
                .flat_map(|(&[r, g, b], &a)| [r, g, b, (a * 255.0).round() as u8])
                .collect();
            egui::ColorImage::from_rgba_unmultiplied(size, &rgba)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumbnails_cover_their_box() {
        assert_eq!(cover_size(360, 203), (160, 90));
        // Ultrawide: as tall as the box, wider than it.
        assert_eq!(cover_size(360, 151), (215, 90));
        assert_eq!(cover_size(40, 20), (40, 20));
    }

    #[test]
    fn stored_preview_tone_maps_the_same() {
        // Shadows to 10,000 nits, with some colours outside BT.709.
        let pixels: Vec<[f32; 3]> = (0..4096)
            .map(|i| {
                let v = (i as f32 / 4095.0).powi(3) * 125.0;
                [v, v * 0.5 - 0.01, v * (i % 7) as f32 / 6.0]
            })
            .collect();
        let image = ScRgbImage {
            width: 64,
            height: 64,
            pixels,
            alpha: None,
            hdr: true,
            sdr_white: None,
        };
        let preview = Preview {
            levels: hdrshot_core::light_levels(&image),
            image,
        };
        let restored = Stored::new(&preview).restore();
        let sdr = SdrOptions::default();
        let before = hdrshot_core::sdr_rgb8(&preview.image, &preview.levels, &sdr);
        let after = hdrshot_core::sdr_rgb8(&restored.image, &restored.levels, &sdr);
        let worst = before
            .iter()
            .zip(&after)
            .map(|(&a, &b)| a.abs_diff(b))
            .max();
        assert!(worst <= Some(1), "{worst:?}");
    }
}
