//! Small previews of the listed images, decoded one at a time on a
//! background thread so adding many files doesn't stall the window.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};

use half::f16;
use hdrshot_core::{LightLevels, Preview, ScRgbImage, SdrOptions};

/// Largest preview kept per file. The tooltip shows it at this size.
pub const PREVIEW_SIZE: (u32, u32) = (360, 220);
/// Largest thumbnail in the file list.
pub const THUMB_SIZE: (u32, u32) = (56, 32);

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
                        let (w, h) = THUMB_SIZE;
                        let (tw, th) = hdrshot_core::resize::fit_box(
                            preview.image.width,
                            preview.image.height,
                            w * 2,
                            h * 2,
                        );
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

/// A preview as kept for its row: half floats and 8-bit alpha take less
/// than half the memory of the float preview, with no visible difference
/// once tone-mapped to 8 bits.
pub struct Stored {
    width: u32,
    height: u32,
    pixels: Vec<[f16; 3]>,
    alpha: Option<Vec<u8>>,
    hdr: bool,
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
    let image = &preview.image;
    let rgb = hdrshot_core::sdr_rgb8(image, &preview.levels, sdr);
    let size = [image.width as usize, image.height as usize];
    let color = match &image.alpha {
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
    };
    ctx.load_texture(name, color, egui::TextureOptions::LINEAR)
}

#[cfg(test)]
mod tests {
    use super::*;

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
