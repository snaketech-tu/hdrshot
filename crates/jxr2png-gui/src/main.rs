#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod assoc;
mod convert;
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
    let viewport = egui::ViewportBuilder::default()
        .with_title("jxr2png")
        .with_app_id("jxr2png")
        .with_inner_size([560.0, 640.0])
        .with_min_inner_size([420.0, 480.0])
        .with_drag_and_drop(true)
        .with_icon(icon());
    if let Err(e) = window::run(viewport, App::new) {
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
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mode: Mode::Both,
            save_to_folder: false,
            output_dir: None,
            copy_to_clipboard: true,
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
    notice: Option<String>,
    clipboard: SharedClipboard,
    toast: Option<Toast>,
    /// Messages from clipboard copies running in the background.
    copied: (Sender<String>, Receiver<String>),
    /// Whether jxr2png opens `.jxr` files; rechecked when the window
    /// regains focus, since the choice is made in the system's settings.
    is_default_app: bool,
    was_focused: bool,
}

struct Toast {
    text: String,
    /// `egui` time at which it disappears.
    until: f64,
}

const TOAST_SECONDS: f64 = 3.0;

const SETTINGS_KEY: &str = "settings";

impl App {
    fn new(ctx: &egui::Context) -> Self {
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
            notice: None,
            clipboard: SharedClipboard::default(),
            toast: None,
            copied: channel(),
            is_default_app: assoc::SUPPORTED && assoc::is_default(),
            was_focused: true,
        };
        if assoc::SUPPORTED {
            assoc::refresh();
        }
        // Files passed on the command line, e.g. by "Open with" or by
        // dropping them onto the program's icon.
        app.add_paths(std::env::args_os().skip(1).map(PathBuf::from));
        app
    }

    fn running(&self) -> bool {
        self.job.is_some()
    }

    fn add_paths(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        let mut skipped = 0;
        for path in paths {
            if path.is_dir() {
                let mut found = Vec::new();
                collect_jxr(&path, &mut found);
                found.sort();
                for file in found {
                    self.add_file(file);
                }
            } else if is_jxr(&path) {
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
            self.files.push(Entry {
                path,
                status: Status::Waiting,
            });
        }
    }

    fn convert_options(&self) -> ConvertOptions {
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
            // Replacing matches v1.0.0 until the GUI asks what to do.
            existing: Existing::Replace,
            keep_date: true,
        }
    }

    fn can_convert(&self) -> bool {
        !self.files.is_empty()
            && !self.running()
            && (!self.settings.save_to_folder || self.settings.output_dir.is_some())
    }

    fn start(&mut self, ctx: &egui::Context) {
        let files: Vec<_> = self
            .files
            .iter_mut()
            .enumerate()
            .map(|(i, e)| {
                e.status = Status::Waiting;
                (i, e.path.clone())
            })
            .collect();
        self.progress = (0, files.len());
        self.notice = None;
        let clipboard = self
            .settings
            .copy_to_clipboard
            .then(|| self.clipboard.clone());
        self.job = Some(Job::start(
            files,
            self.convert_options(),
            clipboard,
            ctx.clone(),
        ));
    }

    fn poll_job(&mut self, ctx: &egui::Context) {
        let Some(job) = self.job.take() else { return };
        let mut done = false;
        while let Ok(event) = job.events.try_recv() {
            match event {
                Event::Started(i) => self.files[i].status = Status::Converting,
                Event::Finished(i, result) => {
                    self.files[i].status = match result {
                        Ok(outputs) => Status::Done(outputs),
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
            }
        } else {
            self.job = Some(job);
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
        let mut remove = None;
        let mut copy = None;
        let stripe = ui.visuals().faint_bg_color;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for (i, entry) in self.files.iter().enumerate() {
                    let (rect, _) = ui
                        .allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::hover());
                    if i % 2 == 0 {
                        ui.painter().rect_filled(rect, 4.0, stripe);
                    }
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
                            remove = Some(i);
                        }
                        status_ui(ui, &entry.status);
                        if let Status::Done(outputs) = &entry.status
                            && ui
                                .small_button("Copy for Discord")
                                .on_hover_text(
                                    "Copy the PNG (the HDR one if made) to paste into Discord. \
                                     The copy leaves out the cICP chunk, which Discord turns yellow.",
                                )
                                .clicked()
                        {
                            copy = Some(outputs.clone());
                        }
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            let name = entry.path.file_name().unwrap_or_default().to_string_lossy();
                            ui.add(egui::Label::new(name.as_ref()).truncate())
                                .on_hover_text(entry.path.display().to_string());
                        });
                    });
                }
            });
        if let Some(i) = remove {
            self.files.remove(i);
        }
        if let Some(outputs) = copy {
            convert::copy_in_background(
                self.clipboard.clone(),
                outputs,
                self.copied.0.clone(),
                ui.ctx().clone(),
            );
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
                let label = |h: Option<u32>| match h {
                    None => "Source".to_owned(),
                    Some(h) => format!("{h}p"),
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
                         aspect ratio. Smaller images are never scaled up.",
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
            .show(ui, |ui| self.tuning_ui(ui));
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
                if ui
                    .add_enabled(!self.files.is_empty(), egui::Button::new("Clear list"))
                    .clicked()
                {
                    self.files.clear();
                    self.progress = (0, 0);
                    self.notice = None;
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
                        .min_size(Vec2::new(150.0, 32.0))
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

        let margin = egui::Margin::same(16);
        let panel_fill = ui.visuals().panel_fill;
        egui::Panel::bottom("actions")
            .frame(egui::Frame::new().inner_margin(margin).fill(panel_fill))
            .show(ui, |ui| {
                ui.add_enabled_ui(!self.running(), |ui| self.settings_ui(ui));
                ui.separator();
                self.action_bar(ui);
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
        self.toast_ui(&ctx);
    }

    fn on_exit(&mut self) {
        storage::save(SETTINGS_KEY, &self.settings);
    }
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
            let saved: Vec<String> = [&outputs.hdr, &outputs.sdr]
                .into_iter()
                .flatten()
                .map(|p| p.display().to_string())
                .collect();
            ui.label(RichText::new("Done").color(ok_color(ui)))
                .on_hover_text(format!("Saved:\n{}", saved.join("\n")));
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

fn is_jxr(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("jxr") || e.eq_ignore_ascii_case("wdp"))
}

fn collect_jxr(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jxr(&path, out);
        } else if is_jxr(&path) {
            out.push(path);
        }
    }
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
