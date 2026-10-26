#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod actions;
mod adding;
mod assoc;
mod background;
mod convert;
mod dragout;
mod files;
mod icons;
mod jobs;
#[cfg(target_os = "macos")]
mod macos;
mod previews;
mod selection;
mod settings;
mod single;
mod startup;
mod storage;
mod taskbar;
mod theme;
mod toasts;
mod tray;
mod update;
mod update_ui;
mod viewer;
mod watching;
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

use adding::drop_overlay;
use convert::{Copied, Event, Job, SharedClipboard, TooBig};
use files::*;
use jobs::*;
use previews::{Previews, THUMB_SIZE, Thumb};
use selection::Selection;
use settings::*;
use toasts::*;
use update_ui::*;
use viewer::Viewer;
use watching::Watching;

/// The app icon, 256 px. Windows also gets `assets/icon.ico` embedded in
/// the executable by `build.rs`.
const ICON_PNG: &[u8] = include_bytes!("../assets/icon.png");

/// Extensions offered in the open dialog, in both cases for platforms whose
/// dialogs match them exactly.
const INPUT_EXTENSIONS: [&str; 24] = [
    "jxr", "wdp", "exr", "hdr", "png", "jxl", "jpg", "jpeg", "heic", "heif", "hif", "avif", "JXR",
    "WDP", "EXR", "HDR", "PNG", "JXL", "JPG", "JPEG", "HEIC", "HEIF", "HIF", "AVIF",
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
    // From Explorer's "Convert with hdrshot", to convert the files right
    // away, or the start at sign-in.
    let launch = match files.first().and_then(|f| f.to_str()) {
        Some(single::CONVERT_FLAG) => single::Launch::Convert,
        Some(startup::TRAY_FLAG) => single::Launch::Tray,
        _ => single::Launch::Open,
    };
    if launch != single::Launch::Open {
        files.remove(0);
    }
    let listener = match single::claim(&files, launch) {
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
        .with_inner_size([560.0, 640.0])
        .with_min_inner_size([420.0, 480.0])
        .with_drag_and_drop(true)
        .with_icon(icon());
    if let Err(e) = window::run(viewport, |ctx| {
        App::new(ctx, files, launch, listener, updated)
    }) {
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
    /// The running batch, or the last one.
    batch: Batch,
    /// Whether the banner summing up the last batch shows.
    banner: bool,
    /// The original converted last, whose row keeps its Copy for Discord
    /// button.
    last_converted: Option<PathBuf>,
    /// A note about files just added, such as unsupported ones skipped.
    notice: Option<String>,
    clipboard: SharedClipboard,
    toast: Option<Toast>,
    /// Messages from clipboard copies running in the background.
    copied: (Sender<Copied>, Receiver<Copied>),
    /// A copy over Discord's size limit, and whether a smaller one is
    /// being made.
    too_big: Option<(TooBig, bool)>,
    /// Whether the Settings page shows in place of the file list.
    settings_open: bool,
    /// Whether hdrshot opens `.jxr` files; rechecked when the window
    /// regains focus, since the choice is made in the system's settings.
    is_default_app: bool,
    was_focused: bool,
    /// Rows picked by clicking them, for the keyboard shortcuts and the
    /// right-click menu.
    selection: Selection,
    /// Rows just removed, until their Undo expires.
    removed: Option<Removed>,
    /// The large preview, while open.
    viewer: Option<Viewer>,
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
    watching: Option<Watching>,
    /// Watching paused from the header, until resumed or hdrshot restarts.
    watch_paused: bool,
    /// Images to convert right away: new ones from the watch folder and
    /// those from "Convert with hdrshot" in another launch.
    watched: (Sender<PathBuf>, Receiver<PathBuf>),
    /// Those not converted yet, waiting for the running batch to end.
    waiting: Vec<PathBuf>,
    /// `egui` time when the last of them arrived.
    last_arrival: f64,
    /// The icon in the tray, while hdrshot keeps running with its window
    /// closed.
    tray: Option<tray::Tray>,
    /// Whether the icon couldn't be added; not tried again until restart.
    tray_failed: bool,
    tray_events: (Sender<tray::TrayEvent>, Receiver<tray::TrayEvent>),
    /// Whether the window is hidden, with hdrshot in the tray.
    hidden: bool,
    /// Quit chosen from the tray icon's menu.
    quit: bool,
}

/// Tooltip for the Copy for Discord buttons; the README's "Sharing on
/// Discord" section has the details.
fn discord_copy_hint() -> String {
    let mut hint = "Copy the image (the HDR one if made) as a PNG that Discord shows \
         correctly.\nDiscord turns HDR PNGs with a cICP chunk yellow, shows AVIF too bright and \
         Ultra HDR JPEGs in SDR, so the copy is an HDR PNG without that chunk.\nYour saved files \
         are unchanged."
        .to_owned();
    if dragout::SUPPORTED {
        hint.push_str(
            "\nDragging the row into Discord, a browser or a folder gives the same file.",
        );
    }
    hint
}

const SETTINGS_KEY: &str = "settings";

impl App {
    /// Opens with `files` in the list, converting them right away if
    /// started from "Convert with hdrshot", or hidden in the tray if
    /// started at sign-in.
    fn new(
        ctx: &egui::Context,
        files: Vec<PathBuf>,
        launch: single::Launch,
        listener: Option<single::Listener>,
        updated: bool,
    ) -> Self {
        let settings: Settings = storage::load(SETTINGS_KEY).unwrap_or_default();
        let hidden = launch == single::Launch::Tray && settings.tray && tray::SUPPORTED;
        let (open, opened) = channel();
        let watched = channel();
        if let Some(listener) = listener {
            listener.serve(open.clone(), watched.0.clone(), ctx.clone());
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
            batch: Batch::default(),
            banner: false,
            last_converted: None,
            notice: None,
            clipboard: SharedClipboard::default(),
            toast: None,
            copied: channel(),
            too_big: None,
            settings_open: false,
            is_default_app: assoc::SUPPORTED && assoc::is_default(),
            was_focused: true,
            selection: Selection::default(),
            removed: None,
            viewer: None,
            opened,
            previews: Previews::new(ctx.clone()),
            shown: None,
            asking: None,
            update: Update::Idle,
            updates: channel(),
            logo,
            watching: None,
            watch_paused: false,
            watched,
            waiting: Vec::new(),
            last_arrival: 0.0,
            tray: None,
            tray_failed: false,
            tray_events: channel(),
            hidden,
            quit: false,
        };
        if assoc::SUPPORTED {
            assoc::refresh();
        }
        if app.settings.start_with_windows {
            startup::refresh();
        }
        if launch == single::Launch::Convert {
            app.waiting = files;
        } else {
            app.add_paths(files);
        }
        if updated {
            app.show_toast(format!("Updated to hdrshot {}", update::CURRENT), ctx);
        }
        if app.settings.theme != ThemeChoice::System {
            app.settings.theme.apply(ctx);
        }
        app.weekly_update_check(ctx);
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
        self.settings_open = false;
        self.show_window(ctx);
    }

    fn running(&self) -> bool {
        self.job.is_some()
    }

    /// Makes hdrshot the app that opens `.jxr` files, or opens the
    /// system's settings to finish that.
    fn make_default_app(&mut self, ctx: &egui::Context) {
        match assoc::make_default() {
            Ok(assoc::Outcome::Done) => {
                self.is_default_app = true;
                self.show_toast("hdrshot now opens .jxr files".into(), ctx);
            }
            Ok(assoc::Outcome::Confirm) => {
                self.show_toast("In Settings, choose hdrshot for .jxr to finish".into(), ctx)
            }
            Err(e) => self.show_toast(e, ctx),
        }
    }

    /// Whether hdrshot opens `.jxr` files, with a button to make it, on the
    /// Settings page.
    fn default_app_ui(&mut self, ui: &mut egui::Ui) {
        if self.is_default_app {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                icons::done(ui.painter(), rect.center(), 7.0, ok_color(ui));
                ui.label("hdrshot opens .jxr files");
            });
            return;
        }
        if ui
            .button("Open .jxr files with hdrshot")
            .on_hover_text(if cfg!(windows) {
                "Make hdrshot the app that opens .jxr files when you double-click them, \
                 list it under Open with for .wdp, .exr, .jxl, .hdr and .heic files, and add \
                 Convert with hdrshot to their right-click menu"
            } else {
                "Make hdrshot the app that opens .jxr files when you double-click them, \
                 and list it under Open with for .wdp, .exr, .jxl, .hdr and .heic files"
            })
            .clicked()
        {
            let ctx = ui.ctx().clone();
            self.make_default_app(&ctx);
        }
    }

    /// The app's icon and name, with the Settings button and, while the
    /// watch folder is on, its pill.
    fn header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add(egui::Image::new((self.logo.id(), Vec2::splat(26.0))).corner_radius(6.0));
            ui.add_space(2.0);
            ui.label(RichText::new("hdrshot").font(theme::semibold(18.0)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::icon_button(ui, icons::settings)
                    .on_hover_text("Settings")
                    .clicked()
                {
                    self.settings_open = true;
                }
                self.watch_pill(ui);
            });
        });
    }
}

impl window::App for App {
    fn ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.poll_job(&ctx);
        let focused = ui.input(|i| i.focused);
        if focused && !self.was_focused {
            if assoc::SUPPORTED {
                self.is_default_app = assoc::is_default();
            }
            // Windows puts the title bar back to the system's theme when
            // any system setting changes.
            if self.settings.theme != ThemeChoice::System {
                self.settings.theme.apply(&ctx);
            }
        }
        self.was_focused = focused;
        // Not the files being dragged out, dropped back onto the window.
        let dropped: Vec<PathBuf> = ui.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .filter(|path| !dragout::is_dragged(path))
                .collect()
        });
        if !dropped.is_empty() {
            self.add_paths(dropped);
            self.settings_open = false;
            self.viewer = None;
        }
        self.shortcuts(&ctx);
        self.receive_opened(&ctx);
        self.sync_watch(&ctx);
        self.receive_watched(&ctx);
        self.sync_tray(&ctx);
        for (path, thumb) in self.previews.finished(&ctx) {
            if let Some(entry) = self.files.iter_mut().find(|e| e.path == path) {
                entry.thumb = thumb;
            }
        }

        let margin = egui::Margin::same(16);
        let panel_fill = ui.visuals().panel_fill;
        let frame = egui::Frame::new().inner_margin(margin).fill(panel_fill);
        if self.settings_open {
            egui::CentralPanel::default()
                .frame(frame)
                .show(ui, |ui| self.settings_page(ui));
        } else {
            egui::Panel::bottom("actions")
                .show_separator_line(false)
                .frame(frame.inner_margin(egui::Margin { top: 4, ..margin }))
                .show(ui, |ui| self.bottom_bar(ui));
            egui::CentralPanel::default().frame(frame).show(ui, |ui| {
                self.header(ui);
                ui.add_space(10.0);
                if self.files.is_empty() {
                    self.empty_ui(ui);
                } else {
                    self.toolbar(ui);
                    ui.add_space(2.0);
                    self.file_list(ui);
                }
            });
        }
        self.viewer_ui(&ctx);
        drop_overlay(&ctx);
        self.existing_dialog(&ctx);
        self.update_dialog(&ctx);
        self.toast_ui(&ctx);
    }

    fn place(&self) -> Option<window::Place> {
        self.settings.window
    }

    fn on_exit(&mut self, place: Option<window::Place>) {
        if place.is_some() {
            self.settings.window = place;
        }
        storage::save(SETTINGS_KEY, &self.settings);
        // Let the file being converted finish, so its temporary file
        // doesn't stay behind in the user's folder.
        if let Some(job) = self.job.take() {
            job.finish();
        }
    }

    fn starts_hidden(&self) -> bool {
        self.hidden
    }

    fn hides_on_close(&self) -> bool {
        self.tray.is_some()
    }

    fn on_hide(&mut self, place: Option<window::Place>) {
        if place.is_some() {
            self.settings.window = place;
        }
        self.hidden_in_tray();
        // hdrshot may now run until Windows shuts down.
        storage::save(SETTINGS_KEY, &self.settings);
    }

    fn wants_quit(&self) -> bool {
        self.quit
    }

    fn taskbar_progress(&self) -> Option<f32> {
        let batch = &self.batch;
        self.running()
            .then(|| batch.done as f32 / batch.total.max(1) as f32)
    }

    fn paste_files(&mut self, files: Vec<PathBuf>) {
        self.add_paths(files);
    }
}

const KOFI_URL: &str = "https://ko-fi.com/snake_tech";

/// "Made with ♥ by snake.tech", centered, opening the Ko-fi page when
/// clicked. The bundled fonts have no heart glyph, so it's drawn. Returns
/// the line's rectangle.
fn credit(ui: &mut egui::Ui, button_width: f32) -> egui::Rect {
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
    // Centered, or on the left in a window too narrow for it to clear the
    // update button on the right.
    let (row, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), size.y), Sense::hover());
    let mut left = row.center().x - size.x / 2.0;
    if left + size.x > row.right() - button_width - 8.0 {
        left = row.left();
    }
    let rect = egui::Rect::from_min_size(egui::pos2(left, row.top()), size);
    let response = ui
        .interact(rect, ui.scope_id().with("credit"), Sense::click())
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
