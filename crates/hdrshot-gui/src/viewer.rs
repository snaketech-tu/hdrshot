//! The large preview: one image as big as the window, tone-mapped to SDR.
//! Clicking a thumbnail or pressing Space opens it. It is decoded when
//! asked and freed when closed, so the list keeps only its small previews.

use std::sync::mpsc::{Receiver, Sender, channel};

use super::*;

/// Largest preview made, in pixels.
const MAX_SIZE: (u32, u32) = (3840, 2160);

/// Room around the picture for the name above it and the hint below.
const TOP: f32 = 52.0;
const BOTTOM: f32 = 40.0;
/// Room on each side for the previous and next buttons.
const SIDE: f32 = 52.0;

pub(crate) struct Viewer {
    path: PathBuf,
    /// The row's small preview, enlarged, until the large one is ready.
    placeholder: Option<egui::TextureHandle>,
    image: Option<egui::TextureHandle>,
    error: Option<String>,
    loader: Loader,
}

/// What the loader makes: an image at most this size, with these options.
struct Request {
    path: PathBuf,
    size: (u32, u32),
    input: InputOptions,
    sdr: SdrOptions,
}

type Loaded = (PathBuf, Result<egui::ColorImage, String>);

/// Makes the large previews on one thread, one at a time. Requests made
/// meanwhile, e.g. by holding an arrow key, are skipped but the last.
/// The thread ends when the preview closes.
struct Loader {
    requests: Sender<Request>,
    loaded: Receiver<Loaded>,
}

impl Loader {
    fn new(ctx: &egui::Context) -> Self {
        let (requests, waiting) = channel::<Request>();
        let (sender, loaded) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            while let Ok(mut request) = waiting.recv() {
                while let Ok(newer) = waiting.try_recv() {
                    request = newer;
                }
                let Request {
                    path,
                    size,
                    input,
                    sdr,
                } = request;
                let image = hdrshot_core::preview(&path, &input, size.0, size.1)
                    .map(|preview| previews::sdr_image(&preview, &sdr))
                    .map_err(|e| e.to_string());
                if sender.send((path, image)).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        });
        Self { requests, loaded }
    }
}

impl App {
    /// Shows the large preview of row `row`, with the current settings.
    pub(crate) fn open_viewer(&mut self, row: usize, ctx: &egui::Context) {
        let Some(entry) = self.files.get(row) else {
            return;
        };
        let options = self.convert_options(Existing::Skip);
        let placeholder = match &entry.thumb {
            Thumb::Ready { preview, .. } => Some(previews::texture(
                ctx,
                "viewer placeholder",
                &preview.restore(),
                &options.sdr,
            )),
            Thumb::Loading | Thumb::Failed => None,
        };
        let screen = ctx.content_rect().size() * ctx.pixels_per_point();
        let size = (
            (screen.x as u32).clamp(1, MAX_SIZE.0),
            (screen.y as u32).clamp(1, MAX_SIZE.1),
        );
        let loader = match self.viewer.take() {
            Some(viewer) => viewer.loader,
            None => Loader::new(ctx),
        };
        // Fails only if the thread panicked; the spinner then stays.
        let _ = loader.requests.send(Request {
            path: entry.path.clone(),
            size,
            input: options.input,
            sdr: options.sdr,
        });
        self.viewer = Some(Viewer {
            path: entry.path.clone(),
            placeholder,
            image: None,
            error: None,
            loader,
        });
        self.selection.set(row);
    }

    /// The row shown, if it is still in the list.
    fn viewer_row(&self) -> Option<usize> {
        let viewer = self.viewer.as_ref()?;
        self.files.iter().position(|e| e.path == viewer.path)
    }

    /// Shows the previous (`step` -1) or next (1) image.
    fn viewer_step(&mut self, step: isize, ctx: &egui::Context) {
        if let Some(row) = self.viewer_row()
            && let Some(next) = row.checked_add_signed(step)
            && next < self.files.len()
        {
            self.open_viewer(next, ctx);
        }
    }

    /// Escape or Space closes the preview; Left and Right show the
    /// previous and next image.
    pub(crate) fn viewer_shortcuts(&mut self, ctx: &egui::Context) {
        let pressed = |key| ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, key));
        if pressed(egui::Key::Escape) || pressed(egui::Key::Space) {
            self.viewer = None;
        } else if pressed(egui::Key::ArrowLeft) || pressed(egui::Key::ArrowUp) {
            self.viewer_step(-1, ctx);
        } else if pressed(egui::Key::ArrowRight) || pressed(egui::Key::ArrowDown) {
            self.viewer_step(1, ctx);
        }
    }

    /// Covers the window with the large preview while it is open.
    pub(crate) fn viewer_ui(&mut self, ctx: &egui::Context) {
        let row = self.viewer_row();
        let count = self.files.len();
        let Some(viewer) = &mut self.viewer else {
            return;
        };
        // Images of rows shown before are dropped.
        while let Ok((path, result)) = viewer.loader.loaded.try_recv() {
            if path != viewer.path {
                continue;
            }
            match result {
                Ok(image) => {
                    viewer.image =
                        Some(ctx.load_texture("viewer", image, egui::TextureOptions::LINEAR));
                    viewer.placeholder = None;
                }
                Err(e) => viewer.error = Some(e),
            }
        }
        let screen = ctx.content_rect();
        let mut close = false;
        let mut step = 0;
        egui::Area::new(egui::Id::unique("viewer"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                // Dark in both themes, as photo viewers are.
                let backdrop = ui.allocate_rect(screen, Sense::click());
                close |= backdrop.clicked();
                let painter = ui.painter();
                painter.rect_filled(screen, 0.0, Color32::from_rgb(12, 13, 16));
                let light = Color32::from_rgb(232, 234, 238);
                let dim = Color32::from_rgb(150, 155, 165);

                let name = viewer
                    .path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                let name_pos = egui::pos2(screen.left() + 18.0, screen.top() + TOP / 2.0);
                let name_rect = painter.text(
                    name_pos,
                    egui::Align2::LEFT_CENTER,
                    &name,
                    theme::semibold(15.0),
                    light,
                );
                if let Some(row) = row
                    && count > 1
                {
                    painter.text(
                        name_rect.right_center() + Vec2::new(10.0, 0.0),
                        egui::Align2::LEFT_CENTER,
                        format!("{} of {count}", row + 1),
                        egui::FontId::proportional(13.0),
                        dim,
                    );
                }
                let close_rect = egui::Rect::from_center_size(
                    egui::pos2(screen.right() - 30.0, screen.top() + TOP / 2.0),
                    Vec2::splat(30.0),
                );
                if overlay_button(ui, close_rect, icons::cross, "Close (Esc)").clicked() {
                    close = true;
                }

                let area = egui::Rect::from_min_max(
                    screen.min + Vec2::new(SIDE, TOP),
                    screen.max - Vec2::new(SIDE, BOTTOM),
                );
                let shown = viewer.image.as_ref().or(viewer.placeholder.as_ref());
                if let Some(texture) = shown {
                    let [w, h] = texture.size();
                    let size = Vec2::new(w as f32, h as f32);
                    // The large image at most at its own size in pixels,
                    // the small one enlarged to fill.
                    let mut scale = (area.width() / size.x).min(area.height() / size.y);
                    if viewer.image.is_some() {
                        scale = scale.min(1.0 / ui.ctx().pixels_per_point());
                    }
                    let rect = egui::Rect::from_center_size(area.center(), size * scale);
                    // Clicks on the picture don't close it.
                    ui.allocate_rect(rect, Sense::click());
                    egui::Image::new((texture.id(), rect.size()))
                        .corner_radius(4.0)
                        .paint_at(ui, rect);
                }
                if let Some(error) = &viewer.error {
                    ui.painter().text(
                        area.center(),
                        egui::Align2::CENTER_CENTER,
                        format!("Can't show this image: {error}"),
                        egui::FontId::proportional(14.0),
                        light,
                    );
                } else if viewer.image.is_none() {
                    let spinner = egui::Rect::from_center_size(
                        egui::pos2(area.center().x, area.bottom() - 18.0),
                        Vec2::splat(22.0),
                    );
                    ui.put(spinner, egui::Spinner::new().size(20.0).color(light));
                }

                if let Some(row) = row {
                    let middle = area.center().y;
                    let previous = egui::Rect::from_center_size(
                        egui::pos2(screen.left() + SIDE / 2.0, middle),
                        Vec2::splat(36.0),
                    );
                    let next = egui::Rect::from_center_size(
                        egui::pos2(screen.right() - SIDE / 2.0, middle),
                        Vec2::splat(36.0),
                    );
                    if row > 0
                        && overlay_button(ui, previous, icons::back, "Previous image (Left)")
                            .clicked()
                    {
                        step = -1;
                    }
                    if row + 1 < count
                        && overlay_button(ui, next, icons::forward, "Next image (Right)").clicked()
                    {
                        step = 1;
                    }
                }
                ui.painter().text(
                    egui::pos2(screen.center().x, screen.bottom() - BOTTOM / 2.0),
                    egui::Align2::CENTER_CENTER,
                    "SDR preview; the HDR image keeps the full brightness.",
                    egui::FontId::proportional(12.5),
                    dim,
                );
            });
        if close {
            self.viewer = None;
        } else if step != 0 {
            self.viewer_step(step, ctx);
        }
    }
}

/// A round button on the dark preview, with a drawn icon.
fn overlay_button(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    icon: icons::Icon,
    hint: &str,
) -> egui::Response {
    let response = ui
        .allocate_rect(rect, Sense::click())
        .on_hover_text(hint)
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let fill = if response.hovered() {
        Color32::from_white_alpha(40)
    } else {
        Color32::from_white_alpha(16)
    };
    let painter = ui.painter();
    painter.circle_filled(rect.center(), rect.width() / 2.0, fill);
    icon(painter, rect.shrink(rect.width() * 0.3), Color32::WHITE);
    response
}
