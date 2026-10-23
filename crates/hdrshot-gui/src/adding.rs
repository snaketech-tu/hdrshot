//! Adding files: the drop area of the empty window with its tips, the
//! toolbar above the list, and the overlay shown while files are dragged
//! over the window.

use super::*;

/// The input formats, named on the empty window's drop area.
const FORMATS: [&str; 5] = ["JPEG XR", "OpenEXR", "Radiance HDR", "JPEG XL", "HDR PNG"];

/// Names of the tips under the empty drop area, kept in the settings once
/// hidden.
const WATCH_TIP: &str = "watch-folder";
const OPEN_TIP: &str = "open-jxr";

impl App {
    pub(crate) fn add_paths(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        let mut skipped = 0;
        for path in paths {
            if path.is_dir() {
                for file in hdrshot_core::find_inputs(&path, true) {
                    self.add_file(file);
                }
            } else if hdrshot_core::is_supported(&path) {
                self.add_file(path);
            } else {
                skipped += 1;
            }
        }
        self.notice = (skipped > 0).then(|| {
            format!(
                "Skipped {skipped} file{} that {} not a supported image.",
                if skipped == 1 { "" } else { "s" },
                if skipped == 1 { "is" } else { "are" }
            )
        });
    }

    pub(crate) fn add_file(&mut self, path: PathBuf) {
        if !self.files.iter().any(|e| e.path == path) {
            let size = hdrshot_core::input::dimensions(&path).ok();
            let bytes = std::fs::metadata(&path).ok().map(|m| m.len());
            self.previews.request(&path);
            self.files.push(Entry {
                info: input_info(&path, size, bytes),
                path,
                status: Status::Waiting,
                size,
                saved: String::new(),
                thumb: Thumb::Loading,
            });
        }
    }

    /// Asks for images with the system's file dialog and adds them.
    pub(crate) fn choose_files(&mut self) {
        if let Some(paths) = rfd::FileDialog::new()
            .set_title("Choose HDR images")
            .add_filter("HDR images", &INPUT_EXTENSIONS)
            .pick_files()
        {
            self.add_paths(paths);
        }
    }

    /// The empty window: a large drop area, with tips under it until they
    /// are followed or hidden.
    pub(crate) fn empty_ui(&mut self, ui: &mut egui::Ui) {
        // Laid out from the bottom, so the drop area takes what's left.
        let layout = egui::Layout::bottom_up(egui::Align::Min).with_cross_justify(true);
        ui.with_layout(layout, |ui| {
            if self.shows_tip(OPEN_TIP) {
                let text = if cfg!(windows) {
                    "Double-click a screenshot to open it here, or right-click it to \
                     convert it."
                } else {
                    "Double-click a screenshot to open it here."
                };
                match tip(
                    ui,
                    icons::image,
                    "Open .jxr files with hdrshot",
                    text,
                    "Set up",
                ) {
                    Some(true) => {
                        let ctx = ui.ctx().clone();
                        self.make_default_app(&ctx);
                    }
                    Some(false) => self.settings.hidden_tips.push(OPEN_TIP.into()),
                    None => {}
                }
            }
            if self.shows_tip(WATCH_TIP)
                && let Some(dir) = watching::default_watch_dir()
            {
                match tip(
                    ui,
                    icons::eye,
                    "Watch the Game Bar folder",
                    "Convert new HDR screenshots as soon as you take them, while \
                     hdrshot is open.",
                    "Turn on",
                ) {
                    Some(true) => {
                        let ctx = ui.ctx().clone();
                        self.show_toast(format!("Watching {}", dir.display()), &ctx);
                        self.settings.watch_dir = Some(dir);
                        self.settings.watch = true;
                    }
                    Some(false) => self.settings.hidden_tips.push(WATCH_TIP.into()),
                    None => {}
                }
            }
            let height = ui.available_height().max(180.0);
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                self.drop_area(ui, height);
            });
        });
    }

    /// Whether the tip `name` applies and isn't hidden.
    fn shows_tip(&self, name: &str) -> bool {
        if self.settings.hidden_tips.iter().any(|t| t == name) {
            return false;
        }
        match name {
            WATCH_TIP => {
                let default = watching::default_watch_dir();
                !self.settings.watch
                    && default.is_some()
                    && (self.settings.watch_dir.is_none() || self.settings.watch_dir == default)
            }
            OPEN_TIP => assoc::SUPPORTED && !self.is_default_app,
            _ => false,
        }
    }

    /// The large drop area of the empty window, which opens the file dialog
    /// when clicked.
    fn drop_area(&mut self, ui: &mut egui::Ui, height: f32) {
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::click());
        let visuals = ui.visuals();
        let accent = visuals.selection.bg_fill;
        let hovered = response.hovered();
        let painter = ui.painter();
        if hovered {
            painter.rect_filled(rect, 12.0, accent.gamma_multiply(0.08));
            painter.rect_stroke(rect, 12.0, Stroke::new(1.5, accent), StrokeKind::Inside);
        } else {
            painter.rect_filled(rect, 12.0, visuals.faint_bg_color);
            let stroke = Stroke::new(1.5, visuals.weak_text_color().gamma_multiply(0.45));
            dashed_outline(painter, rect.shrink(1.0), 12.0, stroke);
        }

        let center = rect.center() - Vec2::new(0.0, 16.0);
        let circle = center - Vec2::new(0.0, 46.0);
        painter.circle_filled(
            circle,
            30.0,
            accent.gamma_multiply(if hovered { 0.24 } else { 0.14 }),
        );
        icons::image(
            painter,
            egui::Rect::from_center_size(circle, Vec2::splat(28.0)),
            accent,
        );
        painter.text(
            center,
            egui::Align2::CENTER_CENTER,
            "Drop HDR images or folders here",
            theme::semibold(16.0),
            visuals.text_color(),
        );
        let paste = if cfg!(target_os = "macos") {
            "Cmd+V"
        } else {
            "Ctrl+V"
        };
        painter.text(
            center + Vec2::new(0.0, 24.0),
            egui::Align2::CENTER_CENTER,
            format!("or click to choose files. {paste} pastes copied files."),
            egui::FontId::proportional(13.0),
            visuals.weak_text_color(),
        );
        format_tags(
            painter,
            center + Vec2::new(0.0, 58.0),
            visuals,
            rect.width(),
        );

        if response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            self.choose_files();
        }
    }

    /// The line above the list: how many files it holds, Add files and
    /// Clear.
    pub(crate) fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let total = self.files.len();
            let converted = self
                .files
                .iter()
                .filter(|e| matches!(e.status, Status::Done(_)))
                .count();
            let mut count = images(total);
            if converted > 0 {
                count = format!("{count}, {converted} converted");
            }
            ui.label(RichText::new(count).weak());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::icon_text_button(ui, icons::plus, "Add files")
                    .on_hover_text("Choose more images. You can also drop them on the window.")
                    .clicked()
                {
                    self.choose_files();
                }
                ui.add_enabled_ui(!self.running(), |ui| {
                    ui.menu_button("Clear", |ui| {
                        if ui.button("All files").clicked() {
                            self.clear_all();
                        }
                        if ui
                            .add_enabled(converted > 0, egui::Button::new("Converted files"))
                            .on_hover_text("Keep the files still waiting or failed")
                            .clicked()
                        {
                            self.clear_finished();
                        }
                    })
                    .response
                    .on_hover_text("Remove files from the list");
                });
            });
        });
    }
}

/// While files are dragged over the window, covers it with a large target
/// saying they will be added. Dropping anywhere adds them.
pub(crate) fn drop_overlay(ctx: &egui::Context) {
    if ctx.input(|i| i.raw.hovered_files.is_empty()) {
        return;
    }
    let layer = egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("drop overlay"));
    let painter = ctx.layer_painter(layer);
    let style = ctx.global_style();
    let visuals = &style.visuals;
    let accent = visuals.selection.bg_fill;
    let screen = ctx.content_rect();
    painter.rect_filled(screen, 0.0, visuals.panel_fill);
    let target = screen.shrink(12.0);
    painter.rect_filled(target, 14.0, accent.gamma_multiply(0.10));
    dashed_outline(&painter, target, 14.0, Stroke::new(2.0, accent));
    let center = target.center();
    let circle = center - Vec2::new(0.0, 44.0);
    painter.circle_filled(circle, 34.0, accent.gamma_multiply(0.2));
    icons::drop(
        &painter,
        egui::Rect::from_center_size(circle, Vec2::splat(30.0)),
        accent,
    );
    painter.text(
        center + Vec2::new(0.0, 8.0),
        egui::Align2::CENTER_CENTER,
        "Release to add",
        theme::semibold(18.0),
        visuals.text_color(),
    );
    painter.text(
        center + Vec2::new(0.0, 34.0),
        egui::Align2::CENTER_CENTER,
        "Nothing is converted until you press Convert.",
        egui::FontId::proportional(13.0),
        visuals.weak_text_color(),
    );
}

/// The input formats as small tags in a row centered on `center`, as many
/// as fit in `width`.
fn format_tags(painter: &egui::Painter, center: egui::Pos2, visuals: &egui::Visuals, width: f32) {
    let font = egui::FontId::proportional(11.5);
    let color = visuals.weak_text_color();
    let gap = 6.0;
    let mut galleys: Vec<_> = FORMATS
        .iter()
        .map(|f| painter.layout_no_wrap((*f).to_owned(), font.clone(), color))
        .collect();
    let row_width = |galleys: &[std::sync::Arc<egui::Galley>]| {
        galleys.iter().map(|g| g.size().x + 16.0 + gap).sum::<f32>() - gap
    };
    while galleys.len() > 1 && row_width(&galleys) > width - 32.0 {
        galleys.pop();
    }
    let mut x = center.x - row_width(&galleys) / 2.0;
    for galley in galleys {
        let size = galley.size() + Vec2::new(16.0, 6.0);
        let rect = egui::Rect::from_min_size(egui::pos2(x, center.y - size.y / 2.0), size);
        painter.rect(
            rect,
            size.y / 2.0,
            visuals.widgets.inactive.bg_fill,
            Stroke::NONE,
            StrokeKind::Inside,
        );
        painter.galley(rect.center() - galley.size() / 2.0, galley, color);
        x = rect.right() + gap;
    }
}

/// A card with an icon, a title, a line of text and a button, plus a
/// cross to hide it. Returns `Some(true)` when the button is clicked and
/// `Some(false)` when the cross is.
fn tip(
    ui: &mut egui::Ui,
    icon: icons::Icon,
    title: &str,
    text: &str,
    button: &str,
) -> Option<bool> {
    let mut clicked = None;
    theme::card(ui, |ui| {
        ui.horizontal(|ui| {
            let accent = theme::accent(ui);
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(32.0), Sense::hover());
            ui.painter()
                .circle_filled(rect.center(), 16.0, accent.gamma_multiply(0.14));
            icon(
                ui.painter(),
                egui::Rect::from_center_size(rect.center(), Vec2::splat(16.0)),
                accent,
            );
            ui.add_space(4.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::small_icon_button(ui, icons::cross)
                    .on_hover_text("Don't show this again")
                    .clicked()
                {
                    clicked = Some(false);
                }
                if ui.button(button).clicked() {
                    clicked = Some(true);
                }
                ui.add_space(4.0);
                ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.label(RichText::new(title).font(theme::semibold(13.5)));
                    ui.label(RichText::new(text).size(12.5).weak());
                });
            });
        });
    });
    // Above the card, as the tips are laid out from the bottom.
    ui.add_space(8.0);
    clicked
}
