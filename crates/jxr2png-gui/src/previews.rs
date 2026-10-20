//! Small previews of the listed images, decoded one at a time on a
//! background thread so adding many files doesn't stall the window.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};

use jxr2png_core::{Preview, SdrOptions};

/// Largest preview kept per file. The tooltip shows it at this size.
pub const PREVIEW_SIZE: (u32, u32) = (360, 220);
/// Largest thumbnail in the file list.
pub const THUMB_SIZE: (u32, u32) = (56, 32);

pub enum Thumb {
    Loading,
    Ready {
        preview: Box<Preview>,
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
                let preview = jxr2png_core::preview(&path, &Default::default(), w, h)
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
                        let (tw, th) = jxr2png_core::resize::fit_box(
                            preview.image.width,
                            preview.image.height,
                            w * 2,
                            h * 2,
                        );
                        let small = jxr2png_core::resize::resize(&preview.image, tw, th);
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
                            preview: Box::new(preview),
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

/// `preview` tone-mapped like the SDR PNG, as a texture.
pub fn texture(
    ctx: &egui::Context,
    name: &str,
    preview: &Preview,
    sdr: &SdrOptions,
) -> egui::TextureHandle {
    let image = &preview.image;
    let rgb = jxr2png_core::sdr_rgb8(image, &preview.levels, sdr);
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
