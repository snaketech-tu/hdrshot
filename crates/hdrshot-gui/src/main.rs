#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod assoc;
mod convert;
mod files;
mod jobs;
#[cfg(target_os = "macos")]
mod macos;
mod previews;
mod settings;
mod single;
mod storage;
mod taskbar;
mod theme;
mod toasts;
mod update;
mod update_ui;
mod window;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{Receiver, Sender, channel};

use egui::{Color32, RichText, Sense, Stroke, StrokeKind, Vec2};
use hdrshot_core::{
    ConvertOptions, Existing, HdrFormat, HdrOptions, InputOptions, Naming, OutputMode, Outputs,
    SdrFormat, SdrOptions,
};
use serde::{Deserialize, Serialize};

use convert::{Copied, Event, Job, SharedClipboard, TooBig};
use files::*;
use jobs::*;
use previews::{Previews, THUMB_SIZE, Thumb};
use settings::*;
use toasts::*;
use update_ui::*;

/// The app icon, 256 px. Windows also gets `assets/icon.ico` embedded in
/// the executable by `build.rs`.
const ICON_PNG: &[u8] = include_bytes!("../assets/icon.png");

/// Extensions offered in the open dialog, in both cases for platforms whose
/// dialogs match them exactly.
const INPUT_EXTENSIONS: [&str; 12] = [
    "jxr", "wdp", "exr", "hdr", "png", "jxl", "JXR", "WDP", "EXR", "HDR", "PNG", "JXL",
];

fn main() {
    // Files passed on the command line, e.g. by "Open with" or by dropping
    // them onto the program's icon.
    let mut files: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    // Started by the old version after an update; it quits right away.
    let updated = files
        .first()
        .is_some_and(|f| f.as_os_str() == update::UPDATED_FLAG);
    if updated {
        files.remove(0);
        single::wait_for_exit(std::time::Duration::from_secs(10));
    }
    update::clean_up();
    let listener = match single::claim(&files) {
        Some(single::Claim::HandedOff) => return,
        Some(single::Claim::First(listener)) => Some(listener),
        None => None,
    };
    // Only once this is the running copy: a second one hands its files off
    // and must not delete what the first made.
    convert::clean_up();
    let viewport = egui::ViewportBuilder::default()
        .with_title(format!("hdrshot {}", env!("CARGO_PKG_VERSION")))
        .with_app_id("hdrshot")
        .with_inner_size([560.0, 760.0])
        .with_min_inner_size([420.0, 480.0])
        .with_drag_and_drop(true)
        .with_icon(icon());
    if let Err(e) = window::run(viewport, |ctx| App::new(ctx, files, listener, updated)) {
        eprintln!("hdrshot: {e}");
        std::process::exit(1);
    }
}

fn icon() -> egui::IconData {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(ICON_PNG));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().expect("bundled icon is a valid PNG");
    let mut rgba = vec![0; reader.output_buffer_size().expect("icon fits in memory")];
    let info = reader
        .next_frame(&mut rgba)
        .expect("bundled icon is a valid PNG");
    assert_eq!(
        info.color_type,
        png::ColorType::Rgba,
        "bundled icon is RGBA"
    );
    rgba.truncate(info.buffer_size());
    egui::IconData {
        rgba,
        width: info.width,
        height: info.height,
    }
}

struct App {
    settings: Settings,
    tuning: Tuning,
    /// Largest output height; `None` keeps the source resolution. Not
    /// remembered, so every session starts at the source resolution.
    max_height: Option<u32>,
    files: Vec<Entry>,
    job: Option<Job>,
    /// Files finished in the running (or last) batch, and its size.
    progress: (usize, usize),
    /// Files in the running (or last) batch whose PNGs all existed already.
    skipped: usize,
    notice: Option<String>,
    clipboard: SharedClipboard,
    toast: Option<Toast>,
    /// Messages from clipboard copies running in the background.
    copied: (Sender<Copied>, Receiver<Copied>),
    /// A copy over Discord's size limit, and whether a smaller one is
    /// being made.
    too_big: Option<(TooBig, bool)>,
    /// Height of the bottom panel without the Advanced settings' scroll
    /// area, and of that area, from the last frame.
    bottom_rest: f32,
    advanced_height: f32,
    /// Whether hdrshot opens `.jxr` files; rechecked when the window
    /// regains focus, since the choice is made in the system's settings.
    is_default_app: bool,
    was_focused: bool,
    /// Row picked by clicking it, for the keyboard shortcuts.
    selected: Option<usize>,
    previews: Previews,
    shown: Option<Shown>,
    /// Files opened while the app was already running, and on macOS all
    /// files opened from Finder.
    opened: Receiver<Vec<PathBuf>>,
    /// A batch waiting for the user to say what to do with PNGs that
    /// already exist.
    asking: Option<Asking>,
    update: Update,
    /// Results of the update check and install, made in the background.
    updates: (Sender<UpdateMessage>, Receiver<UpdateMessage>),
    /// The app icon, shown in the header.
    logo: egui::TextureHandle,
}

/// Tooltip for the Copy for Discord buttons; the README's "Sharing on
/// Discord" section has the details.
const DISCORD_COPY_HINT: &str = "Copy the image (the HDR one if made) as a PNG that Discord shows \
     correctly.\nDiscord turns HDR PNGs with a cICP chunk yellow, shows AVIF too bright and \
     Ultra HDR JPEGs in SDR, so the copy is an HDR PNG without that chunk.\nYour saved files \
     are unchanged.";

/// Height kept for the title, drop area and a few rows of the file list
/// when the Advanced settings are open.
const MIN_LIST_AREA: f32 = 220.0;
/// The Advanced settings never shrink below this; they scroll instead.
const MIN_ADVANCED_HEIGHT: f32 = 120.0;

const SETTINGS_KEY: &str = "settings";

impl App {
    fn new(
        ctx: &egui::Context,
        files: Vec<PathBuf>,
        listener: Option<single::Listener>,
        updated: bool,
    ) -> Self {
        let settings = storage::load(SETTINGS_KEY).unwrap_or_default();
        let (open, opened) = channel();
        if let Some(listener) = listener {
            listener.serve(open.clone(), ctx.clone());
        }
        #[cfg(target_os = "macos")]
        macos::receive_opened_files(open.clone(), ctx.clone());
        drop(open);
        theme::apply(ctx);
        let logo = {
            let icon = icon();
            ctx.load_texture(
                "logo",
                egui::ColorImage::from_rgba_unmultiplied(
                    [icon.width as usize, icon.height as usize],
                    &icon.rgba,
                ),
                egui::TextureOptions::LINEAR,
            )
        };
        let mut app = Self {
            settings,
            tuning: Tuning::default(),
            max_height: None,
            files: Vec::new(),
            job: None,
            progress: (0, 0),
            skipped: 0,
            notice: None,
            clipboard: SharedClipboard::default(),
            toast: None,
            copied: channel(),
            too_big: None,
            bottom_rest: 330.0,
            advanced_height: 0.0,
            is_default_app: assoc::SUPPORTED && assoc::is_default(),
            was_focused: true,
            selected: None,
            opened,
            previews: Previews::new(ctx.clone()),
            shown: None,
            asking: None,
            update: Update::Idle,
            updates: channel(),
            logo,
        };
        if assoc::SUPPORTED {
            assoc::refresh();
        }
        app.add_paths(files);
        if updated {
            app.show_toast(format!("Updated to hdrshot {}", update::CURRENT), ctx);
        }
        app
    }

    /// Adds files opened in another launch of the app and brings the
    /// window to the front. Adding is safe mid-batch: new rows go at the end
    /// and the batch only refers to the rows it started with.
    fn receive_opened(&mut self, ctx: &egui::Context) {
        let batches: Vec<_> = self.opened.try_iter().collect();
        if batches.is_empty() {
            return;
        }
        for files in batches {
            self.add_paths(files);
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    fn running(&self) -> bool {
        self.job.is_some()
    }

    fn default_app_ui(&mut self, ui: &mut egui::Ui) {
        if !assoc::SUPPORTED {
            return;
        }
        if self.is_default_app {
            ui.weak("Opens .jxr files");
            return;
        }
        if ui
            .small_button("Open .jxr files with hdrshot")
            .on_hover_text(
                "Make hdrshot the app that opens .jxr files when you double-click them, \
                 and list it under Open with for .wdp, .exr, .jxl and .hdr files",
            )
            .clicked()
        {
            let ctx = ui.ctx().clone();
            match assoc::make_default() {
                Ok(assoc::Outcome::Done) => {
                    self.is_default_app = true;
                    self.show_toast("hdrshot now opens .jxr files".into(), &ctx);
                }
                Ok(assoc::Outcome::Confirm) => self.show_toast(
                    "In Settings, choose hdrshot for .jxr to finish".into(),
                    &ctx,
                ),
                Err(e) => self.show_toast(e, &ctx),
            }
        }
    }

    fn action_bar(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let (done, total) = self.progress;
        if self.running() {
            ui.add(
                egui::ProgressBar::new(done as f32 / total.max(1) as f32)
                    .text(format!("Converting {} of {total}", (done + 1).min(total))),
            );
        } else if let Some(notice) = &self.notice {
            ui.label(notice);
        } else if total > 0 && done == total {
            ui.label(format!(
                "Converted {total} image{}.",
                if total == 1 { "" } else { "s" }
            ));
        }

        ui.horizontal(|ui| {
            if !self.running() {
                let finished = self
                    .files
                    .iter()
                    .any(|e| matches!(e.status, Status::Done(_)));
                ui.add_enabled_ui(!self.files.is_empty(), |ui| {
                    ui.menu_button("Clear", |ui| {
                        if ui.button("All files").clicked() {
                            self.files.clear();
                            self.selected = None;
                            self.progress = (0, 0);
                            self.notice = None;
                        }
                        if ui
                            .add_enabled(finished, egui::Button::new("Converted files"))
                            .on_hover_text("Keep the files still waiting or failed")
                            .clicked()
                        {
                            self.clear_finished();
                        }
                    })
                    .response
                    .on_hover_text("Remove files from the list");
                });
                let failed = self.failed();
                if !failed.is_empty()
                    && ui
                        .add_enabled(self.can_convert(), egui::Button::new("Retry failed"))
                        .on_hover_text("Convert the files that failed again")
                        .clicked()
                {
                    self.start_some(failed, &ctx);
                }
                if let Some(dir) = self.output_folder()
                    && ui
                        .button("Open folder")
                        .on_hover_text(dir.display().to_string())
                        .clicked()
                {
                    reveal(&dir);
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let big = |text: &str| {
                    egui::Button::new(RichText::new(text).strong().size(15.0))
                        .min_size(Vec2::new(150.0, 34.0))
                        .corner_radius(8.0)
                };
                if let Some(job) = &self.job {
                    let stopping = job.is_cancelled();
                    let label = if stopping { "Stopping…" } else { "Stop" };
                    if ui
                        .add_enabled(!stopping, big(label))
                        .on_hover_text("Stop after the current image")
                        .clicked()
                    {
                        job.cancel();
                    }
                    return;
                }
                let label = match self.files.len() {
                    0 => "Convert".to_owned(),
                    1 => "Convert 1 image".to_owned(),
                    n => format!("Convert {n} images"),
                };
                let button = theme::primary(ui, theme::primary_button(&label));
                let response = ui.add_enabled(self.can_convert(), button);
                let response = if self.settings.save_to_folder && self.settings.output_dir.is_none()
                {
                    response.on_disabled_hover_text("Choose a folder to save into first")
                } else {
                    response
                };
                if response.clicked() {
                    self.start(&ctx);
                }
            });
        });
    }
}

impl window::App for App {
    fn ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.poll_job(&ctx);
        let focused = ui.input(|i| i.focused);
        if focused && !self.was_focused && assoc::SUPPORTED {
            self.is_default_app = assoc::is_default();
        }
        self.was_focused = focused;
        let dropped: Vec<PathBuf> = ui.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        if !dropped.is_empty() {
            self.add_paths(dropped);
        }
        self.shortcuts(&ctx);
        self.receive_opened(&ctx);
        for (path, thumb) in self.previews.finished(&ctx) {
            if let Some(entry) = self.files.iter_mut().find(|e| e.path == path) {
                entry.thumb = thumb;
            }
        }

        let margin = egui::Margin::same(16);
        let panel_fill = ui.visuals().panel_fill;
        self.advanced_height = 0.0;
        let bottom = egui::Panel::bottom("actions")
            .show_separator_line(false)
            .frame(egui::Frame::new().inner_margin(margin).fill(panel_fill))
            .show(ui, |ui| {
                ui.add_enabled_ui(!self.running(), |ui| self.settings_ui(ui));
                ui.add_space(4.0);
                self.action_bar(ui);
                // Room for the update button, which is taller than the
                // credit line it sits on.
                ui.add_space(10.0);
                let row = credit(ui);
                self.update_button(ui, row);
            });
        self.bottom_rest = bottom.response.rect.height() - self.advanced_height;
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(margin).fill(panel_fill))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add(
                        egui::Image::new((self.logo.id(), Vec2::splat(30.0))).corner_radius(7.0),
                    );
                    ui.spacing_mut().item_spacing.x = 10.0;
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.label(RichText::new("hdrshot").strong().size(19.0));
                        ui.label(RichText::new("HDR image tool").size(12.0).weak());
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        self.default_app_ui(ui);
                    });
                });
                ui.add_space(8.0);
                if self.files.is_empty() {
                    let height = ui.available_height().max(140.0);
                    self.drop_zone(ui, height);
                } else {
                    self.drop_zone(ui, 64.0);
                    self.file_list(ui);
                }
            });
        self.existing_dialog(&ctx);
        self.update_dialog(&ctx);
        self.toast_ui(&ctx);
    }

    fn on_exit(&mut self) {
        storage::save(SETTINGS_KEY, &self.settings);
        // Let the file being converted finish, so its temporary file
        // doesn't stay behind in the user's folder.
        if let Some(job) = self.job.take() {
            job.finish();
        }
    }

    fn taskbar_progress(&self) -> Option<f32> {
        let (done, total) = self.progress;
        self.running().then(|| done as f32 / total.max(1) as f32)
    }

    fn paste_files(&mut self, files: Vec<PathBuf>) {
        self.add_paths(files);
    }
}

const KOFI_URL: &str = "https://ko-fi.com/snake_tech";

/// "Made with ♥ by snake.tech", centered, opening the Ko-fi page when
/// clicked. The bundled fonts have no heart glyph, so it's drawn. Returns
/// the line's rectangle.
fn credit(ui: &mut egui::Ui) -> egui::Rect {
    let font = egui::FontId::proportional(11.5);
    let heart_size = 9.0;
    let gap = 4.0;
    let color = ui.visuals().weak_text_color();
    let before = ui
        .painter()
        .layout_no_wrap("Made with".to_owned(), font.clone(), color);
    let after = ui
        .painter()
        .layout_no_wrap("by snake.tech".to_owned(), font, color);
    let size = Vec2::new(
        before.size().x + after.size().x + heart_size + 2.0 * gap,
        before.size().y,
    );
    ui.vertical_centered(|ui| {
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        let response = response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(KOFI_URL);
        let ink = if response.hovered() {
            ui.visuals().text_color()
        } else {
            color
        };
        let painter = ui.painter();
        let mut x = rect.left();
        painter.galley_with_override_text_color(rect.left_top(), before.clone(), ink);
        x += before.size().x + gap;
        let center = egui::pos2(x + heart_size / 2.0, rect.center().y);
        paint_heart(painter, center, heart_size, Color32::from_rgb(220, 70, 90));
        x += heart_size + gap;
        painter.galley_with_override_text_color(egui::pos2(x, rect.top()), after.clone(), ink);
        if response.clicked() {
            ui.ctx().open_url(egui::OpenUrl::new_tab(KOFI_URL));
        }
        rect
    })
    .inner
}

/// A filled heart `size` points wide, centered on `center`.
fn paint_heart(painter: &egui::Painter, center: egui::Pos2, size: f32, color: Color32) {
    // Two circles for the lobes and a triangle for the point.
    let r = size / 4.0;
    let top = center.y - size * 0.15;
    painter.circle_filled(egui::pos2(center.x - r, top), r, color);
    painter.circle_filled(egui::pos2(center.x + r, top), r, color);
    painter.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(center.x - 2.0 * r, top + r * 0.25),
            egui::pos2(center.x + 2.0 * r, top + r * 0.25),
            egui::pos2(center.x, center.y + size * 0.45),
        ],
        color,
        Stroke::NONE,
    ));
}
