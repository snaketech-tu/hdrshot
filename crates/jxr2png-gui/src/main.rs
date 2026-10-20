#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod assoc;
mod convert;
mod single;
mod storage;
mod window;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{Receiver, Sender, channel};

use egui::{Color32, RichText, Sense, Stroke, StrokeKind, Vec2};
use jxr2png_core::{ConvertOptions, Existing, HdrOptions, OutputMode, Outputs, SdrOptions};
use serde::{Deserialize, Serialize};

use convert::{Event, Job, SharedClipboard};

/// The app icon, 256 px. Windows also gets `assets/icon.ico` embedded in
/// the executable by `build.rs`.
const ICON_PNG: &[u8] = include_bytes!("../assets/icon.png");

fn main() {
    // Files passed on the command line, e.g. by "Open with" or by dropping
    // them onto the program's icon.
    let files: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let listener = match single::claim(&files) {
        Some(single::Claim::HandedOff) => return,
        Some(single::Claim::First(listener)) => Some(listener),
        None => None,
    };
    let viewport = egui::ViewportBuilder::default()
        .with_title("jxr2png")
        .with_app_id("jxr2png")
        .with_inner_size([560.0, 640.0])
        .with_min_inner_size([420.0, 480.0])
        .with_drag_and_drop(true)
        .with_icon(icon());
    if let Err(e) = window::run(viewport, |ctx| App::new(ctx, files, listener)) {
        eprintln!("jxr2png: {e}");
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

/// What the user picked last time. Tuning options are deliberately not
/// remembered: every session starts out matching the original screenshots.
#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    mode: Mode,
    save_to_folder: bool,
    output_dir: Option<PathBuf>,
    copy_to_clipboard: bool,
    keep_date: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mode: Mode::Both,
            save_to_folder: false,
            output_dir: None,
            copy_to_clipboard: true,
            keep_date: true,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum Mode {
    Hdr,
    Sdr,
    Both,
}

impl From<Mode> for OutputMode {
    fn from(mode: Mode) -> Self {
        match mode {
            Mode::Hdr => Self::Hdr,
            Mode::Sdr => Self::Sdr,
            Mode::Both => Self::Both,
        }
    }
}

/// Optional tuning, off by default.
struct Tuning {
    limit_hdr_peak: bool,
    hdr_peak: f32,
    sdr_white: f32,
    fixed_sdr_peak: bool,
    sdr_peak: f32,
}

impl Default for Tuning {
    fn default() -> Self {
        Self {
            limit_hdr_peak: false,
            hdr_peak: 1000.0,
            sdr_white: SdrOptions::default().white_nits,
            fixed_sdr_peak: false,
            sdr_peak: 1000.0,
        }
    }
}

impl Tuning {
    fn is_default(&self) -> bool {
        !self.limit_hdr_peak
            && !self.fixed_sdr_peak
            && self.sdr_white == SdrOptions::default().white_nits
    }
}

enum Status {
    Waiting,
    Converting,
    Done(Outputs),
    Failed(String),
}

struct Entry {
    path: PathBuf,
    status: Status,
    /// Source width and height, if the file's header could be read.
    size: Option<(u32, u32)>,
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
    copied: (Sender<String>, Receiver<String>),
    /// Whether jxr2png opens `.jxr` files; rechecked when the window
    /// regains focus, since the choice is made in the system's settings.
    is_default_app: bool,
    was_focused: bool,
    /// Row picked by clicking it, for the keyboard shortcuts.
    selected: Option<usize>,
    /// Files opened while the app was already running.
    opened: Option<Receiver<Vec<PathBuf>>>,
    /// A batch waiting for the user to say what to do with PNGs that
    /// already exist.
    asking: Option<Asking>,
}

struct Asking {
    indices: Vec<usize>,
    /// The output files that already exist.
    existing: Vec<PathBuf>,
}

/// Something done to one row of the file list.
enum RowAction {
    Select(usize),
    Open(PathBuf),
    ShowInFolder(PathBuf),
    Copy(Outputs),
    Retry(usize),
    Remove(usize),
}

struct Toast {
    text: String,
    /// `egui` time at which it disappears.
    until: f64,
}

const TOAST_SECONDS: f64 = 3.0;

const SETTINGS_KEY: &str = "settings";

impl App {
    fn new(ctx: &egui::Context, files: Vec<PathBuf>, listener: Option<single::Listener>) -> Self {
        let settings = storage::load(SETTINGS_KEY).unwrap_or_default();
        ctx.global_style_mut(|style| {
            style.spacing.item_spacing = Vec2::new(8.0, 8.0);
            style.spacing.button_padding = Vec2::new(10.0, 4.0);
        });
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
            is_default_app: assoc::SUPPORTED && assoc::is_default(),
            was_focused: true,
            selected: None,
            opened: listener.map(|l| l.serve(ctx.clone())),
            asking: None,
        };
        if assoc::SUPPORTED {
            assoc::refresh();
        }
        app.add_paths(files);
        app
    }

    /// Adds files opened in another launch of the app and brings the
    /// window to the front. Adding is safe mid-batch: new rows go at the end
    /// and the batch only refers to the rows it started with.
    fn receive_opened(&mut self, ctx: &egui::Context) {
        let Some(opened) = &self.opened else { return };
        let batches: Vec<_> = opened.try_iter().collect();
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

    fn add_paths(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        let mut skipped = 0;
        for path in paths {
            if path.is_dir() {
                for file in jxr2png_core::find_jxr(&path, true) {
                    self.add_file(file);
                }
            } else if jxr2png_core::is_jxr(&path) {
                self.add_file(path);
            } else {
                skipped += 1;
            }
        }
        self.notice = (skipped > 0).then(|| {
            format!(
                "Skipped {skipped} file{} that {} not .jxr.",
                if skipped == 1 { "" } else { "s" },
                if skipped == 1 { "is" } else { "are" }
            )
        });
    }

    fn add_file(&mut self, path: PathBuf) {
        if !self.files.iter().any(|e| e.path == path) {
            let size = jxr2png_core::dimensions(&path).ok();
            self.files.push(Entry {
                path,
                status: Status::Waiting,
                size,
            });
        }
    }

    fn convert_options(&self, existing: Existing) -> ConvertOptions {
        let t = &self.tuning;
        ConvertOptions {
            mode: self.settings.mode.into(),
            output_dir: if self.settings.save_to_folder {
                self.settings.output_dir.clone()
            } else {
                None
            },
            hdr: HdrOptions {
                peak_nits: t.limit_hdr_peak.then_some(t.hdr_peak),
            },
            sdr: SdrOptions {
                white_nits: t.sdr_white,
                peak_nits: t.fixed_sdr_peak.then_some(t.sdr_peak),
            },
            max_height: self.max_height,
            existing,
            keep_date: self.settings.keep_date,
        }
    }

    fn can_convert(&self) -> bool {
        !self.files.is_empty()
            && !self.running()
            && self.asking.is_none()
            && (!self.settings.save_to_folder || self.settings.output_dir.is_some())
    }

    /// Converts every listed file.
    fn start(&mut self, ctx: &egui::Context) {
        self.start_some((0..self.files.len()).collect(), ctx);
    }

    /// Converts the files at `indices`, leaving the other rows as they are.
    /// If some of their PNGs already exist, asks first what to do.
    fn start_some(&mut self, indices: Vec<usize>, ctx: &egui::Context) {
        if indices.is_empty() {
            return;
        }
        let options = self.convert_options(Existing::Skip);
        let existing = existing_outputs(indices.iter().map(|&i| &self.files[i].path), &options);
        if existing.is_empty() {
            // Nothing to overwrite, so the choice doesn't matter.
            self.run(indices, Existing::Skip, ctx);
        } else {
            self.asking = Some(Asking { indices, existing });
        }
    }

    fn run(&mut self, indices: Vec<usize>, existing: Existing, ctx: &egui::Context) {
        let files: Vec<_> = indices
            .into_iter()
            .map(|i| {
                let entry = &mut self.files[i];
                entry.status = Status::Waiting;
                (i, entry.path.clone())
            })
            .collect();
        self.progress = (0, files.len());
        self.skipped = 0;
        self.notice = None;
        let clipboard = self
            .settings
            .copy_to_clipboard
            .then(|| self.clipboard.clone());
        self.job = Some(Job::start(
            files,
            self.convert_options(existing),
            clipboard,
            ctx.clone(),
        ));
    }

    fn failed(&self) -> Vec<usize> {
        (0..self.files.len())
            .filter(|&i| matches!(self.files[i].status, Status::Failed(_)))
            .collect()
    }

    fn remove(&mut self, i: usize) {
        self.files.remove(i);
        self.selected = after_removal(self.selected, i);
        self.notice = None;
    }

    /// Removes the files that converted, keeping the rest to retry or add to.
    fn clear_finished(&mut self) {
        let selected = self.selected.map(|i| self.files[i].path.clone());
        self.files.retain(|e| !matches!(e.status, Status::Done(_)));
        self.selected = selected.and_then(|p| self.files.iter().position(|e| e.path == p));
        self.progress = (0, 0);
        self.notice = None;
    }

    /// Delete removes the selected row, Enter converts, and the arrow keys
    /// move the selection; not while a text field has focus or a batch runs.
    fn shortcuts(&mut self, ctx: &egui::Context) {
        if self.running() || self.asking.is_some() || ctx.memory(|m| m.focused().is_some()) {
            return;
        }
        let pressed = |key| ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, key));
        if let Some(i) = self.selected
            && (pressed(egui::Key::Delete) || pressed(egui::Key::Backspace))
        {
            self.remove(i);
        }
        if pressed(egui::Key::Enter) && self.can_convert() {
            self.start(ctx);
        }
        let last = self.files.len().checked_sub(1);
        if pressed(egui::Key::ArrowDown) {
            self.selected = match self.selected {
                Some(i) => last.map(|l| (i + 1).min(l)),
                None => last.map(|_| 0),
            };
        }
        if pressed(egui::Key::ArrowUp) {
            self.selected = match self.selected {
                Some(i) => Some(i.saturating_sub(1)),
                None => last,
            };
        }
        if pressed(egui::Key::Escape) {
            self.selected = None;
        }
    }

    fn poll_job(&mut self, ctx: &egui::Context) {
        let Some(job) = self.job.take() else { return };
        let mut done = false;
        while let Ok(event) = job.events.try_recv() {
            match event {
                Event::Started(i) => self.files[i].status = Status::Converting,
                Event::Finished(i, result) => {
                    self.files[i].status = match result {
                        Ok(outputs) => {
                            if outputs.hdr.is_none() && outputs.sdr.is_none() {
                                self.skipped += 1;
                            }
                            Status::Done(outputs)
                        }
                        Err(e) => Status::Failed(e),
                    };
                    self.progress.0 += 1;
                }
                Event::Done(copied) => {
                    done = true;
                    if let Some(text) = copied {
                        self.show_toast(text, ctx);
                    }
                }
            }
        }
        if done {
            let failed = self
                .files
                .iter()
                .filter(|e| matches!(e.status, Status::Failed(_)))
                .count();
            if job.is_cancelled() {
                self.notice = Some("Stopped.".into());
            } else if failed > 0 {
                self.notice = Some(format!(
                    "{failed} file{} could not be converted.",
                    if failed == 1 { "" } else { "s" }
                ));
            } else if self.skipped > 0 {
                let (n, s) = (self.skipped, if self.skipped == 1 { "" } else { "s" });
                self.notice = Some(if n == self.progress.1 {
                    format!("Skipped {n} file{s}: the PNGs already exist.")
                } else {
                    format!("Skipped {n} file{s} whose PNGs already exist.")
                });
            }
        } else {
            self.job = Some(job);
        }
    }

    /// Asks once per batch what to do with PNGs that already exist.
    fn existing_dialog(&mut self, ctx: &egui::Context) {
        let Some(asking) = &self.asking else { return };
        let mut choice = None;
        let mut cancel = false;
        let modal = egui::Modal::new(egui::Id::unique("existing"))
            .frame(egui::Frame::popup(&ctx.global_style()).inner_margin(16))
            .show(ctx, |ui| {
                ui.set_max_width(360.0);
                let count = asking.existing.len();
                let title = if count == 1 {
                    let name = asking.existing[0].file_name().unwrap_or_default();
                    format!("{} already exists", name.to_string_lossy())
                } else {
                    format!("{count} PNGs already exist")
                };
                ui.label(RichText::new(title).strong().size(15.0));
                ui.label("What should happen to the PNGs that are already there?");
                if count > 1 {
                    let names: Vec<String> = asking
                        .existing
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect();
                    ui.weak("Show files").on_hover_text(names.join("\n"));
                }
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui
                        .button("Replace")
                        .on_hover_text("Overwrite them with the new PNGs")
                        .clicked()
                    {
                        choice = Some(Existing::Replace);
                    }
                    if ui
                        .button("Keep both")
                        .on_hover_text("Save the new PNGs with a number, e.g. shot_hdr (2).png")
                        .clicked()
                    {
                        choice = Some(Existing::KeepBoth);
                    }
                    if ui
                        .button("Skip")
                        .on_hover_text("Leave them alone and only make the PNGs that are missing")
                        .clicked()
                    {
                        choice = Some(Existing::Skip);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        cancel = ui.button("Cancel").clicked();
                    });
                });
            });
        if let Some(existing) = choice {
            let indices = self.asking.take().map(|a| a.indices).unwrap_or_default();
            self.run(indices, existing, ctx);
        } else if cancel || modal.should_close() {
            self.asking = None;
        }
    }

    fn show_toast(&mut self, text: String, ctx: &egui::Context) {
        self.toast = Some(Toast {
            text,
            until: ctx.input(|i| i.time) + TOAST_SECONDS,
        });
    }

    fn toast_ui(&mut self, ctx: &egui::Context) {
        while let Ok(text) = self.copied.1.try_recv() {
            self.show_toast(text, ctx);
        }
        let Some(toast) = &self.toast else { return };
        let left = toast.until - ctx.input(|i| i.time);
        if left <= 0.0 {
            self.toast = None;
            return;
        }
        // Fade out over the last half second.
        let opacity = (left / 0.5).min(1.0) as f32;
        egui::Area::new(egui::Id::unique("toast"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_TOP, Vec2::new(0.0, 56.0))
            .interactable(false)
            .show(ctx, |ui| {
                ui.set_opacity(opacity);
                let visuals = ui.visuals();
                egui::Frame::new()
                    .fill(visuals.extreme_bg_color)
                    .stroke(visuals.window_stroke)
                    .corner_radius(8.0)
                    .shadow(visuals.popup_shadow)
                    .inner_margin(egui::Margin::symmetric(14, 10))
                    .show(ui, |ui| {
                        ui.label(RichText::new(&toast.text).size(14.0));
                    });
            });
        ctx.request_repaint();
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
            .small_button("Open .jxr files with jxr2png")
            .on_hover_text("Make jxr2png the app that opens .jxr files when you double-click them")
            .clicked()
        {
            let ctx = ui.ctx().clone();
            match assoc::make_default() {
                Ok(assoc::Outcome::Done) => {
                    self.is_default_app = true;
                    self.show_toast("jxr2png now opens .jxr files".into(), &ctx);
                }
                Ok(assoc::Outcome::Confirm) => self.show_toast(
                    "In Settings, choose jxr2png for .jxr to finish".into(),
                    &ctx,
                ),
                Err(e) => self.show_toast(e, &ctx),
            }
        }
    }

    /// Folder the last batch wrote to, if there is a single one.
    fn output_folder(&self) -> Option<PathBuf> {
        let mut dirs = self.files.iter().filter_map(|e| match &e.status {
            Status::Done(o) => o
                .hdr
                .as_ref()
                .or(o.sdr.as_ref())?
                .parent()
                .map(Path::to_path_buf),
            _ => None,
        });
        let first = dirs.next()?;
        dirs.all(|d| d == first).then_some(first)
    }

    fn drop_zone(&mut self, ui: &mut egui::Ui, height: f32) {
        let hovering = ui.input(|i| !i.raw.hovered_files.is_empty());
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::click());
        let enabled = !self.running();
        let visuals = ui.visuals();
        let accent = visuals.selection.bg_fill;
        let highlight = enabled && (hovering || response.hovered());
        let fill = if highlight {
            accent.gamma_multiply(0.15)
        } else {
            visuals.faint_bg_color
        };
        let stroke = if highlight {
            Stroke::new(2.0, accent)
        } else {
            Stroke::new(1.5, visuals.widgets.noninteractive.bg_stroke.color)
        };
        let painter = ui.painter();
        painter.rect(rect, 10.0, fill, stroke, StrokeKind::Inside);

        let dim = |c: Color32| if enabled { c } else { c.gamma_multiply(0.4) };
        let text_color = dim(visuals.text_color());
        let weak = dim(visuals.weak_text_color());
        let center = rect.center();
        let (title, hint) = if hovering {
            ("Release to add", "")
        } else {
            (
                "Drop .jxr files or folders here",
                "or click to choose files",
            )
        };
        let big = height > 120.0;
        if big {
            // Drawn as shapes: egui's bundled fonts have no arrow glyphs.
            let arrow = Stroke::new(3.0, if highlight { accent } else { weak });
            let tip = center - Vec2::new(0.0, 22.0);
            painter.line_segment([center - Vec2::new(0.0, 48.0), tip], arrow);
            painter.line(
                vec![
                    tip + Vec2::new(-10.0, -10.0),
                    tip,
                    tip + Vec2::new(10.0, -10.0),
                ],
                arrow,
            );
        }
        let title_y = if big { 10.0 } else { -9.0 };
        painter.text(
            center + Vec2::new(0.0, title_y),
            egui::Align2::CENTER_CENTER,
            title,
            egui::FontId::proportional(17.0),
            text_color,
        );
        painter.text(
            center + Vec2::new(0.0, title_y + 22.0),
            egui::Align2::CENTER_CENTER,
            hint,
            egui::FontId::proportional(13.0),
            weak,
        );

        if enabled
            && response
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            && let Some(paths) = rfd::FileDialog::new()
                .set_title("Choose JXR images")
                .add_filter("JPEG XR images", &["jxr", "JXR", "wdp"])
                .pick_files()
        {
            self.add_paths(paths);
        }
    }

    fn file_list(&mut self, ui: &mut egui::Ui) {
        let running = self.running();
        let mut action = None;
        let stripe = ui.visuals().faint_bg_color;
        let selection = ui.visuals().selection.bg_fill.gamma_multiply(0.35);
        let hover = ui.visuals().widgets.hovered.weak_bg_fill;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for (i, entry) in self.files.iter().enumerate() {
                    let (rect, row) = ui
                        .allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::click());
                    if self.selected == Some(i) {
                        ui.painter().rect_filled(rect, 4.0, selection);
                    } else if row.hovered() {
                        ui.painter().rect_filled(rect, 4.0, hover);
                    } else if i % 2 == 0 {
                        ui.painter().rect_filled(rect, 4.0, stripe);
                    }
                    let outputs = match &entry.status {
                        Status::Done(outputs) if outputs.hdr.is_some() || outputs.sdr.is_some() => {
                            Some(outputs)
                        }
                        _ => None,
                    };
                    let primary = outputs.and_then(|o| o.hdr.as_ref().or(o.sdr.as_ref()));
                    if row.clicked() || row.secondary_clicked() {
                        action = Some(RowAction::Select(i));
                    }
                    if row.double_clicked()
                        && let Some(path) = primary
                    {
                        action = Some(RowAction::Open(path.clone()));
                    }
                    row.context_menu(|ui| {
                        if let Some(menu) = row_menu(ui, i, entry, running) {
                            action = Some(menu);
                        }
                    });

                    let inner = egui::UiBuilder::new()
                        .max_rect(rect.shrink2(Vec2::new(8.0, 0.0)))
                        .layout(egui::Layout::right_to_left(egui::Align::Center));
                    ui.scope_builder(inner, |ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        if !running
                            && ui
                                .small_button("×")
                                .on_hover_text("Remove from the list")
                                .clicked()
                        {
                            action = Some(RowAction::Remove(i));
                        }
                        status_ui(ui, &entry.status);
                        if let Some(outputs) = outputs
                            && ui
                                .small_button("Copy for Discord")
                                .on_hover_text(
                                    "Copy the PNG (the HDR one if made) to paste into Discord. \
                                     The copy leaves out the cICP chunk, which Discord turns yellow.",
                                )
                                .clicked()
                        {
                            action = Some(RowAction::Copy(outputs.clone()));
                        }
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            let name = entry.path.file_name().unwrap_or_default().to_string_lossy();
                            ui.add(egui::Label::new(name.as_ref()).truncate().selectable(false))
                                .on_hover_text(entry.path.display().to_string());
                        });
                    });
                }
            });
        let ctx = ui.ctx().clone();
        match action {
            None => {}
            Some(RowAction::Select(i)) => self.selected = Some(i),
            Some(RowAction::Open(path)) => open(&path),
            Some(RowAction::ShowInFolder(path)) => show_in_folder(&path),
            Some(RowAction::Copy(outputs)) => convert::copy_in_background(
                self.clipboard.clone(),
                outputs,
                self.copied.0.clone(),
                ctx,
            ),
            Some(RowAction::Retry(i)) => self.start_some(vec![i], &ctx),
            Some(RowAction::Remove(i)) => self.remove(i),
        }
    }

    fn settings_ui(&mut self, ui: &mut egui::Ui) {
        egui::Grid::new("settings")
            .num_columns(2)
            .spacing([12.0, 10.0])
            .show(ui, |ui| {
                ui.label("Create");
                ui.horizontal(|ui| {
                    let mode = &mut self.settings.mode;
                    ui.selectable_value(mode, Mode::Both, "HDR + SDR")
                        .on_hover_text("An HDR PNG and a tone-mapped SDR PNG for each image");
                    ui.selectable_value(mode, Mode::Hdr, "HDR only")
                        .on_hover_text(
                            "16-bit HDR PNG, shown in HDR by Windows Photos, Chrome and Edge",
                        );
                    ui.selectable_value(mode, Mode::Sdr, "SDR only")
                        .on_hover_text("Tone-mapped 8-bit PNG that looks right everywhere");
                });
                ui.end_row();

                ui.label("Save to");
                ui.vertical(|ui| {
                    let s = &mut self.settings;
                    ui.radio_value(&mut s.save_to_folder, false, "Same folder as each image");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut s.save_to_folder, true, "");
                        let label = match &s.output_dir {
                            Some(dir) => dir.display().to_string(),
                            None => "Choose a folder…".to_owned(),
                        };
                        let button = ui
                            .add(egui::Button::new(label).truncate())
                            .on_hover_text("Choose the folder to save into");
                        if button.clicked() {
                            let mut dialog = rfd::FileDialog::new().set_title("Save PNGs to");
                            if let Some(dir) = &s.output_dir {
                                dialog = dialog.set_directory(dir);
                            }
                            if let Some(dir) = dialog.pick_folder() {
                                s.output_dir = Some(dir);
                                s.save_to_folder = true;
                            }
                        }
                    });
                });
                ui.end_row();

                ui.label("Resolution");
                let label = |h: Option<u32>| {
                    let name = h.map_or_else(|| "Source".to_owned(), |h| format!("{h}p"));
                    match output_size(&self.files, h) {
                        OutputSize::Unknown => name,
                        OutputSize::Same(w, h) => format!("{name} ({w}×{h})"),
                        OutputSize::Varies => format!("{name} (sizes vary)"),
                    }
                };
                egui::ComboBox::from_id_salt("resolution")
                    .selected_text(label(self.max_height))
                    .show_ui(ui, |ui| {
                        for h in [None, Some(2160), Some(1440), Some(1080), Some(720)] {
                            ui.selectable_value(&mut self.max_height, h, label(h));
                        }
                    })
                    .response
                    .on_hover_text(
                        "Scale images taller than this down to this height, keeping the \
                         aspect ratio, so ultrawide images stay ultrawide. Smaller images \
                         are never scaled up. The size in brackets is what the listed \
                         files come out at.",
                    );
                ui.end_row();

                ui.label("Clipboard");
                ui.checkbox(
                    &mut self.settings.copy_to_clipboard,
                    "Copy the last converted image",
                )
                .on_hover_text(
                    "After converting, copy the last PNG to the clipboard as a file \
                     (the HDR one if made), ready to paste into Discord, a browser or a folder",
                );
                ui.end_row();
            });

        let custom = !self.tuning.is_default();
        let title = if custom {
            "Advanced (custom)"
        } else {
            "Advanced"
        };
        egui::CollapsingHeader::new(title)
            .id_salt("advanced")
            .show(ui, |ui| {
                ui.checkbox(&mut self.settings.keep_date, "Keep the capture date")
                    .on_hover_text(
                        "Give each PNG the date of its screenshot, so folders sorted \
                         by date stay in capture order",
                    );
                self.tuning_ui(ui);
            });
    }

    fn tuning_ui(&mut self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new(
                "Not needed normally: by default the PNGs match the original \
                 screenshot. Use these only to fine-tune the result.",
            )
            .weak(),
        );
        let t = &mut self.tuning;
        let mode = self.settings.mode;
        fn nits(value: &mut f32, min: f32) -> egui::DragValue<'_> {
            egui::DragValue::new(value)
                .range(min..=10_000.0)
                .speed(5.0)
                .suffix(" nits")
        }
        egui::Grid::new("tuning")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                ui.add_enabled_ui(mode != Mode::Sdr, |ui| {
                    ui.checkbox(&mut t.limit_hdr_peak, "Limit HDR peak")
                        .on_hover_text(
                            "Roll off highlights brighter than this in the HDR PNG. \
                             Off keeps the original brightness.",
                        );
                });
                ui.add_enabled(
                    mode != Mode::Sdr && t.limit_hdr_peak,
                    nits(&mut t.hdr_peak, 100.0),
                );
                ui.end_row();

                ui.label("SDR white").on_hover_text(
                    "HDR brightness that becomes white in the SDR PNG. \
                     Lower makes the SDR picture brighter.",
                );
                ui.horizontal(|ui| {
                    ui.add(nits(&mut t.sdr_white, 10.0));
                    let default = SdrOptions::default().white_nits;
                    if t.sdr_white != default && ui.small_button("Reset").clicked() {
                        t.sdr_white = default;
                    }
                });
                ui.end_row();

                ui.checkbox(&mut t.fixed_sdr_peak, "Fixed SDR peak")
                    .on_hover_text(
                        "HDR brightness that maps to full SDR white. \
                     Off uses the brightest 0.1% of each image.",
                    );
                ui.add_enabled(t.fixed_sdr_peak, nits(&mut t.sdr_peak, 10.0));
                ui.end_row();
            });
        if !t.is_default() && ui.button("Back to automatic").clicked() {
            *t = Tuning::default();
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
                        .min_size(Vec2::new(130.0, 32.0))
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
                let response = ui.add_enabled(self.can_convert(), big(&label));
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
        if !dropped.is_empty() && !self.running() {
            self.add_paths(dropped);
        }
        self.shortcuts(&ctx);
        self.receive_opened(&ctx);

        let margin = egui::Margin::same(16);
        let panel_fill = ui.visuals().panel_fill;
        egui::Panel::bottom("actions")
            .frame(egui::Frame::new().inner_margin(margin).fill(panel_fill))
            .show(ui, |ui| {
                ui.add_enabled_ui(!self.running(), |ui| self.settings_ui(ui));
                ui.separator();
                self.action_bar(ui);
                ui.add_space(2.0);
                credit(ui);
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(margin).fill(panel_fill))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(RichText::new("jxr2png").strong());
                    ui.label(RichText::new("HDR screenshots to PNG").weak());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        self.default_app_ui(ui);
                    });
                });
                ui.add_space(4.0);
                if self.files.is_empty() {
                    let height = ui.available_height().max(140.0);
                    self.drop_zone(ui, height);
                } else {
                    self.drop_zone(ui, 64.0);
                    self.file_list(ui);
                }
            });
        self.existing_dialog(&ctx);
        self.toast_ui(&ctx);
    }

    fn on_exit(&mut self) {
        storage::save(SETTINGS_KEY, &self.settings);
    }

    fn paste_files(&mut self, files: Vec<PathBuf>) {
        if !self.running() {
            self.add_paths(files);
        }
    }
}

/// The planned output files of `inputs` that already exist.
fn existing_outputs<'a>(
    inputs: impl Iterator<Item = &'a PathBuf>,
    options: &ConvertOptions,
) -> Vec<PathBuf> {
    inputs
        .flat_map(|input| {
            let planned = jxr2png_core::output_paths(input, options);
            [planned.hdr, planned.sdr]
        })
        .flatten()
        .filter(|p| p.exists())
        .collect()
}

/// What size the listed files come out at with a given height limit.
#[derive(Debug, PartialEq)]
enum OutputSize {
    /// No file in the list, or none whose size could be read.
    Unknown,
    Same(u32, u32),
    Varies,
}

fn output_size(files: &[Entry], max_height: Option<u32>) -> OutputSize {
    let mut sizes = files
        .iter()
        .filter_map(|e| e.size)
        .map(|(w, h)| max_height.map_or((w, h), |max| jxr2png_core::resize::fit_height(w, h, max)));
    let Some(first) = sizes.next() else {
        return OutputSize::Unknown;
    };
    if sizes.all(|s| s == first) {
        OutputSize::Same(first.0, first.1)
    } else {
        OutputSize::Varies
    }
}

/// The selected row once row `removed` is gone.
fn after_removal(selected: Option<usize>, removed: usize) -> Option<usize> {
    match selected {
        Some(s) if s == removed => None,
        Some(s) if s > removed => Some(s - 1),
        s => s,
    }
}

/// The right-click menu of a row.
fn row_menu(ui: &mut egui::Ui, i: usize, entry: &Entry, running: bool) -> Option<RowAction> {
    let mut action = None;
    let outputs = match &entry.status {
        Status::Done(outputs) if outputs.hdr.is_some() || outputs.sdr.is_some() => Some(outputs),
        _ => None,
    };
    let hdr = outputs.and_then(|o| o.hdr.clone());
    let sdr = outputs.and_then(|o| o.sdr.clone());
    if ui
        .add_enabled(hdr.is_some(), egui::Button::new("Open HDR PNG"))
        .clicked()
    {
        action = hdr.clone().map(RowAction::Open);
    }
    if ui
        .add_enabled(sdr.is_some(), egui::Button::new("Open SDR PNG"))
        .clicked()
    {
        action = sdr.clone().map(RowAction::Open);
    }
    if let Some(outputs) = outputs
        && ui.button("Copy for Discord").clicked()
    {
        action = Some(RowAction::Copy(outputs.clone()));
    }
    ui.separator();
    if ui.button("Show in folder").clicked() {
        // The PNG once there is one, else the JXR itself.
        let path = hdr.or(sdr).unwrap_or_else(|| entry.path.clone());
        action = Some(RowAction::ShowInFolder(path));
    }
    if !running {
        let again = if outputs.is_some() {
            "Convert again"
        } else {
            "Convert"
        };
        if ui.button(again).clicked() {
            action = Some(RowAction::Retry(i));
        }
        if ui.button("Remove from list").clicked() {
            action = Some(RowAction::Remove(i));
        }
    }
    if action.is_some() {
        ui.close();
    }
    action
}

fn status_ui(ui: &mut egui::Ui, status: &Status) {
    match status {
        Status::Waiting => {
            ui.weak("Waiting");
        }
        Status::Converting => {
            ui.label("Converting");
            ui.spinner();
        }
        Status::Done(outputs) => {
            let list = |paths: &mut dyn Iterator<Item = &PathBuf>| {
                paths
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            let saved = list(&mut [&outputs.hdr, &outputs.sdr].into_iter().flatten());
            let kept = list(&mut outputs.skipped.iter());
            let tooltip = match (saved.is_empty(), kept.is_empty()) {
                (false, true) => format!("Saved:\n{saved}"),
                (true, _) => format!("Already there, left alone:\n{kept}"),
                (false, false) => {
                    format!("Saved:\n{saved}\n\nAlready there, left alone:\n{kept}")
                }
            };
            if saved.is_empty() {
                ui.weak("Skipped").on_hover_text(tooltip);
            } else {
                ui.label(RichText::new("Done").color(ok_color(ui)))
                    .on_hover_text(tooltip);
            }
        }
        Status::Failed(error) => {
            ui.label(RichText::new("Failed").color(ui.visuals().error_fg_color))
                .on_hover_text(error);
        }
    }
}

fn ok_color(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode {
        Color32::from_rgb(120, 200, 120)
    } else {
        Color32::from_rgb(30, 130, 50)
    }
}

const KOFI_URL: &str = "https://ko-fi.com/snake_tech";

/// "Made with ♥ by snake.tech", centered, opening the Ko-fi page when
/// clicked. The bundled fonts have no heart glyph, so it's drawn.
fn credit(ui: &mut egui::Ui) {
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
    });
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

/// Open `dir` in the system's file manager.
fn reveal(dir: &Path) {
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = Command::new(program).arg(dir).spawn();
}

/// Open `file` in the app the system uses for it.
fn open(file: &Path) {
    #[cfg(windows)]
    let mut command = {
        // `start` needs an empty window title before a quoted path.
        let mut command = Command::new("cmd");
        command.args(["/C", "start", ""]).arg(file);
        hide_console(&mut command);
        command
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        command.arg(file);
        command
    };
    #[cfg(not(any(windows, target_os = "macos")))]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(file);
        command
    };
    let _ = command.spawn();
}

/// Show `file` selected in the system's file manager, or its folder where
/// selecting isn't supported.
fn show_in_folder(file: &Path) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Explorer reads its own command line, so the path is quoted by hand.
        let _ = Command::new("explorer")
            .raw_arg(format!("/select,\"{}\"", file.display()))
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("open").arg("-R").arg(file).spawn();
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    if let Some(dir) = file.parent() {
        reveal(dir);
    }
}

/// Keeps `cmd` from flashing a console window.
#[cfg(windows)]
fn hide_console(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(size: Option<(u32, u32)>) -> Entry {
        Entry {
            path: PathBuf::new(),
            status: Status::Waiting,
            size,
        }
    }

    #[test]
    fn existing_outputs_lists_only_files_on_disk() {
        let dir = std::env::temp_dir().join(format!("jxr2png-gui-existing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a_hdr.png"), b"png").unwrap();
        let inputs = [dir.join("a.jxr"), dir.join("b.jxr")];
        let options = ConvertOptions::default();
        assert_eq!(
            existing_outputs(inputs.iter(), &options),
            [dir.join("a_hdr.png")]
        );
        let sdr_only = ConvertOptions {
            mode: OutputMode::Sdr,
            ..Default::default()
        };
        assert!(existing_outputs(inputs.iter(), &sdr_only).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn selection_follows_removal() {
        assert_eq!(after_removal(Some(2), 2), None);
        assert_eq!(after_removal(Some(3), 1), Some(2));
        assert_eq!(after_removal(Some(1), 3), Some(1));
        assert_eq!(after_removal(None, 0), None);
    }

    #[test]
    fn output_size_keeps_ultrawide_aspect() {
        let files = [entry(Some((3440, 1440))), entry(Some((3440, 1440)))];
        assert_eq!(output_size(&files, None), OutputSize::Same(3440, 1440));
        assert_eq!(
            output_size(&files, Some(1080)),
            OutputSize::Same(2580, 1080)
        );
        assert_eq!(
            output_size(&files, Some(2160)),
            OutputSize::Same(3440, 1440)
        );
    }

    #[test]
    fn output_size_varies_or_unknown() {
        let mixed = [entry(Some((3440, 1440))), entry(Some((3840, 2160)))];
        assert_eq!(output_size(&mixed, Some(1080)), OutputSize::Varies);
        // Both are 1440 or less tall, but their widths differ.
        assert_eq!(output_size(&mixed, Some(1440)), OutputSize::Varies);
        assert_eq!(output_size(&[], Some(1080)), OutputSize::Unknown);
        assert_eq!(output_size(&[entry(None)], None), OutputSize::Unknown);
    }
}
