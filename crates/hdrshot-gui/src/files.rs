//! The drop area and the list of files, with their rows and menus.

use super::*;

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
            self.previews.request(&path);
            self.files.push(Entry {
                path,
                status: Status::Waiting,
                size,
                thumb: Thumb::Loading,
            });
        }
    }

    pub(crate) fn failed(&self) -> Vec<usize> {
        (0..self.files.len())
            .filter(|&i| matches!(self.files[i].status, Status::Failed(_)))
            .collect()
    }

    pub(crate) fn remove(&mut self, i: usize) {
        self.files.remove(i);
        self.selected = after_removal(self.selected, i);
        self.notice = None;
    }

    /// Removes the files that converted, keeping the rest to retry or add to.
    pub(crate) fn clear_finished(&mut self) {
        let selected = self.selected.map(|i| self.files[i].path.clone());
        self.files.retain(|e| !matches!(e.status, Status::Done(_)));
        self.selected = selected.and_then(|p| self.files.iter().position(|e| e.path == p));
        self.progress = (0, 0);
        self.notice = None;
    }

    /// Delete removes the selected row, Enter converts, and the arrow keys
    /// move the selection; not while a text field has focus or a batch runs.
    pub(crate) fn shortcuts(&mut self, ctx: &egui::Context) {
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

    /// Folder the last batch wrote to, if there is a single one.
    /// With HDR and SDR subfolders, the folder holding both.
    pub(crate) fn output_folder(&self) -> Option<PathBuf> {
        let dirs: Vec<&Path> = self
            .files
            .iter()
            .filter_map(|e| match &e.status {
                Status::Done(o) => Some([&o.hdr, &o.sdr]),
                _ => None,
            })
            .flatten()
            .flatten()
            .filter_map(|p| p.parent())
            .collect();
        common_folder(&dirs)
            .or_else(|| {
                let parents: Option<Vec<&Path>> = dirs
                    .iter()
                    .map(|d| {
                        let kind = d.file_name()?;
                        (kind == "hdr" || kind == "sdr").then(|| d.parent())?
                    })
                    .collect();
                common_folder(&parents?)
            })
            .map(Path::to_path_buf)
    }

    pub(crate) fn drop_zone(&mut self, ui: &mut egui::Ui, height: f32) {
        let hovering = ui.input(|i| !i.raw.hovered_files.is_empty());
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::click());
        // Files can be added during a batch too: they go at the end of the
        // list, outside the batch.
        let visuals = ui.visuals();
        let accent = visuals.selection.bg_fill;
        let highlight = hovering || response.hovered();
        let fill = if highlight {
            accent.gamma_multiply(0.15)
        } else {
            visuals.faint_bg_color
        };
        let stroke = if highlight {
            Stroke::new(2.0, accent)
        } else {
            Stroke::new(1.5, visuals.weak_text_color().gamma_multiply(0.5))
        };
        let painter = ui.painter();
        painter.rect_filled(rect, 12.0, fill);
        if highlight {
            painter.rect_stroke(rect, 12.0, stroke, StrokeKind::Inside);
        } else {
            dashed_outline(painter, rect.shrink(1.0), 12.0, stroke);
        }

        let text_color = visuals.text_color();
        let weak = visuals.weak_text_color();
        let center = rect.center();
        let (title, hint) = if hovering {
            ("Release to add", "")
        } else {
            (
                "Drop HDR images or folders here",
                "or click to choose files",
            )
        };
        let big = height > 120.0;
        if big {
            // Drawn as shapes: egui's bundled fonts have no arrow glyphs.
            painter.circle_filled(
                center - Vec2::new(0.0, 38.0),
                30.0,
                accent.gamma_multiply(if highlight { 0.25 } else { 0.12 }),
            );
            let arrow = Stroke::new(3.0, accent);
            let tip = center - Vec2::new(0.0, 24.0);
            painter.line_segment([center - Vec2::new(0.0, 52.0), tip], arrow);
            painter.line(
                vec![
                    tip + Vec2::new(-10.0, -10.0),
                    tip,
                    tip + Vec2::new(10.0, -10.0),
                ],
                arrow,
            );
        }
        let title_y = if big { 14.0 } else { -9.0 };
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

        if response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
            && let Some(paths) = rfd::FileDialog::new()
                .set_title("Choose HDR images")
                .add_filter("HDR images", &INPUT_EXTENSIONS)
                .pick_files()
        {
            self.add_paths(paths);
        }
    }

    pub(crate) fn file_list(&mut self, ui: &mut egui::Ui) {
        let running = self.running();
        let mut action = None;
        let mut hovered_thumb = None;
        let card = ui.visuals().faint_bg_color;
        let selection = ui.visuals().selection.bg_fill.gamma_multiply(0.3);
        let hover = ui.visuals().widgets.inactive.weak_bg_fill;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                for (i, entry) in self.files.iter().enumerate() {
                    let (rect, row) = ui
                        .allocate_exact_size(Vec2::new(ui.available_width(), 44.0), Sense::click());
                    let fill = if self.selected == Some(i) {
                        selection
                    } else if row.hovered() {
                        hover
                    } else {
                        card
                    };
                    ui.painter().rect_filled(rect, 8.0, fill);
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
                        .max_rect(rect.shrink2(Vec2::new(10.0, 0.0)))
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
                                .on_hover_text(DISCORD_COPY_HINT)
                                .clicked()
                        {
                            action = Some(RowAction::Copy(entry.path.clone(), outputs.clone()));
                        }
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            if thumb_ui(
                                ui,
                                &entry.thumb,
                                self.shown.as_ref().filter(|s| s.path == entry.path),
                            ) {
                                hovered_thumb = Some(i);
                            }
                            let name = entry.path.file_name().unwrap_or_default().to_string_lossy();
                            ui.add(egui::Label::new(name.as_ref()).truncate().selectable(false))
                                .on_hover_text(entry.path.display().to_string());
                        });
                    });
                }
            });
        let ctx = ui.ctx().clone();
        self.update_shown(hovered_thumb, &ctx);
        match action {
            None => {}
            Some(RowAction::Select(i)) => self.selected = Some(i),
            Some(RowAction::Open(path)) => open(&path),
            Some(RowAction::ShowInFolder(path)) => show_in_folder(&path),
            Some(RowAction::Copy(input, outputs)) => {
                if convert::copy_makes_png(&outputs) {
                    self.show_toast("Making the PNG for Discord…".into(), &ctx);
                }
                let clipboard = self.clipboard.clone();
                let options = self.convert_options(Existing::Skip);
                convert::in_background(self.copied.0.clone(), ctx, move || {
                    convert::copy(&clipboard, &input, &outputs, &options)
                });
            }
            Some(RowAction::Retry(i)) => self.start_some(vec![i], &ctx),
            Some(RowAction::Remove(i)) => self.remove(i),
        }
    }

    /// Prepares the large preview of row `hovered` with the current
    /// settings; it shows from the next frame.
    pub(crate) fn update_shown(&mut self, hovered: Option<usize>, ctx: &egui::Context) {
        let Some(entry) = hovered.map(|i| &self.files[i]) else {
            return;
        };
        let Thumb::Ready { preview, .. } = &entry.thumb else {
            return;
        };
        let sdr = self.convert_options(Existing::Skip).sdr;
        if self
            .shown
            .as_ref()
            .is_some_and(|s| s.path == entry.path && s.sdr == sdr)
        {
            return;
        }
        let auto = SdrOptions::default();
        let preview = preview.restore();
        self.shown = Some(Shown {
            path: entry.path.clone(),
            sdr,
            auto: previews::texture(ctx, "preview", &preview, &auto),
            tuned: (sdr != auto).then(|| previews::texture(ctx, "preview-tuned", &preview, &sdr)),
        });
        ctx.request_repaint();
    }
}

pub(crate) enum Status {
    Waiting,
    Converting,
    Done(Outputs),
    Failed(String),
}

pub(crate) struct Entry {
    pub(crate) path: PathBuf,
    pub(crate) status: Status,
    /// Source width and height, if the file's header could be read.
    pub(crate) size: Option<(u32, u32)>,
    pub(crate) thumb: Thumb,
}

/// The large preview of the row under the mouse, tone-mapped automatically
/// and, if the Advanced settings are changed, with them.
pub(crate) struct Shown {
    pub(crate) path: PathBuf,
    pub(crate) sdr: SdrOptions,
    pub(crate) auto: egui::TextureHandle,
    pub(crate) tuned: Option<egui::TextureHandle>,
}

/// Something done to one row of the file list.
pub(crate) enum RowAction {
    Select(usize),
    Open(PathBuf),
    ShowInFolder(PathBuf),
    /// Copy the PNGs made from a file.
    Copy(PathBuf, Outputs),
    Retry(usize),
    Remove(usize),
}

/// The folder all of `dirs` are, if they are the same one.
pub(crate) fn common_folder<'a>(dirs: &[&'a Path]) -> Option<&'a Path> {
    let first = *dirs.first()?;
    dirs.iter().all(|&d| d == first).then_some(first)
}

/// The selected row once row `removed` is gone.
pub(crate) fn after_removal(selected: Option<usize>, removed: usize) -> Option<usize> {
    match selected {
        Some(s) if s == removed => None,
        Some(s) if s > removed => Some(s - 1),
        s => s,
    }
}

/// The thumbnail at the start of a row, with the large preview `shown` (if
/// it is this row's) when hovered. Returns whether it is hovered.
pub(crate) fn thumb_ui(ui: &mut egui::Ui, thumb: &Thumb, shown: Option<&Shown>) -> bool {
    let box_size = Vec2::new(THUMB_SIZE.0 as f32, THUMB_SIZE.1 as f32);
    let Thumb::Ready { texture, .. } = thumb else {
        let (rect, _) = ui.allocate_exact_size(box_size, Sense::hover());
        if matches!(thumb, Thumb::Loading) {
            ui.painter()
                .rect_filled(rect.shrink(2.0), 3.0, ui.visuals().faint_bg_color);
        }
        return false;
    };
    let [w, h] = texture.size();
    let (w, h) = hdrshot_core::resize::fit_box(w as u32, h as u32, THUMB_SIZE.0, THUMB_SIZE.1);
    let (rect, response) = ui.allocate_exact_size(box_size, Sense::hover());
    let image_rect = egui::Rect::from_center_size(rect.center(), Vec2::new(w as f32, h as f32));
    egui::Image::new((texture.id(), image_rect.size()))
        .corner_radius(2.0)
        .paint_at(ui, image_rect);
    let hovered = response.hovered();
    if let Some(shown) = shown {
        response.on_hover_ui(|ui| {
            let show = |ui: &mut egui::Ui, texture: &egui::TextureHandle| {
                let [w, h] = texture.size();
                ui.image((texture.id(), Vec2::new(w as f32, h as f32)));
            };
            match &shown.tuned {
                Some(tuned) => {
                    ui.label("Automatic");
                    show(ui, &shown.auto);
                    ui.label("With your Advanced settings");
                    show(ui, tuned);
                }
                None => show(ui, &shown.auto),
            }
            ui.weak("SDR preview; the HDR image keeps the full brightness.");
        });
    }
    hovered
}

/// The right-click menu of a row.
pub(crate) fn row_menu(
    ui: &mut egui::Ui,
    i: usize,
    entry: &Entry,
    running: bool,
) -> Option<RowAction> {
    let mut action = None;
    let outputs = match &entry.status {
        Status::Done(outputs) if outputs.hdr.is_some() || outputs.sdr.is_some() => Some(outputs),
        _ => None,
    };
    let hdr = outputs.and_then(|o| o.hdr.clone());
    let sdr = outputs.and_then(|o| o.sdr.clone());
    if ui
        .add_enabled(hdr.is_some(), egui::Button::new("Open HDR image"))
        .clicked()
    {
        action = hdr.clone().map(RowAction::Open);
    }
    if ui
        .add_enabled(sdr.is_some(), egui::Button::new("Open SDR image"))
        .clicked()
    {
        action = sdr.clone().map(RowAction::Open);
    }
    if let Some(outputs) = outputs
        && ui
            .button("Copy for Discord")
            .on_hover_text(DISCORD_COPY_HINT)
            .clicked()
    {
        action = Some(RowAction::Copy(entry.path.clone(), outputs.clone()));
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

pub(crate) fn status_ui(ui: &mut egui::Ui, status: &Status) {
    match status {
        Status::Waiting => {
            theme::pill(ui, "Waiting", ui.visuals().weak_text_color());
        }
        Status::Converting => {
            ui.spinner();
            theme::pill(ui, "Converting", theme::accent(ui));
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
            let mut tooltip = match (saved.is_empty(), kept.is_empty()) {
                (false, true) => format!("Saved:\n{saved}"),
                (true, _) => format!("Already there, left alone:\n{kept}"),
                (false, false) => {
                    format!("Saved:\n{saved}\n\nAlready there, left alone:\n{kept}")
                }
            };
            if outputs.sdr_input {
                tooltip =
                    format!("This is an SDR image, so only the SDR image was made.\n\n{tooltip}");
            }
            if saved.is_empty() {
                theme::pill(ui, "Skipped", ui.visuals().weak_text_color()).on_hover_text(tooltip);
            } else {
                theme::pill(ui, "Done", ok_color(ui)).on_hover_text(tooltip);
            }
        }
        Status::Failed(error) => {
            theme::pill(ui, "Failed", ui.visuals().error_fg_color).on_hover_text(error);
        }
    }
}

/// A rounded rectangle's outline as dashes, for the empty drop zone.
pub(crate) fn dashed_outline(
    painter: &egui::Painter,
    rect: egui::Rect,
    radius: f32,
    stroke: Stroke,
) {
    let mut points = Vec::new();
    let corners = [
        (rect.right_top() + Vec2::new(-radius, radius), -90.0_f32),
        (rect.right_bottom() + Vec2::new(-radius, -radius), 0.0),
        (rect.left_bottom() + Vec2::new(radius, -radius), 90.0),
        (rect.left_top() + Vec2::new(radius, radius), 180.0),
    ];
    for (center, start) in corners {
        for step in 0..=8 {
            let angle = (start + step as f32 * 90.0 / 8.0).to_radians();
            points.push(center + radius * Vec2::new(angle.cos(), angle.sin()));
        }
    }
    points.push(points[0]);
    painter.extend(egui::Shape::dashed_line(&points, stroke, 7.0, 5.0));
}

pub(crate) fn ok_color(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode {
        Color32::from_rgb(120, 200, 120)
    } else {
        Color32::from_rgb(30, 130, 50)
    }
}

/// Open `dir` in the system's file manager.
pub(crate) fn reveal(dir: &Path) {
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
#[cfg(windows)]
pub(crate) fn open(file: &Path) {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    // Straight to the shell: through `cmd /C start`, characters such as `&`
    // in a file name would be read as commands.
    let wide = |s: &std::ffi::OsStr| s.encode_wide().chain([0]).collect::<Vec<u16>>();
    let (verb, file) = (wide("open".as_ref()), wide(file.as_os_str()));
    // SAFETY: both strings are NUL-terminated and outlive the call; COM is
    // set up on the UI thread by winit.
    unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        );
    }
}

/// Open `file` in the app the system uses for it.
#[cfg(not(windows))]
pub(crate) fn open(file: &Path) {
    let program = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = Command::new(program).arg(file).spawn();
}

/// Show `file` selected in the system's file manager, or its folder where
/// selecting isn't supported.
pub(crate) fn show_in_folder(file: &Path) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_follows_removal() {
        assert_eq!(after_removal(Some(2), 2), None);
        assert_eq!(after_removal(Some(3), 1), Some(2));
        assert_eq!(after_removal(Some(1), 3), Some(1));
        assert_eq!(after_removal(None, 0), None);
    }
}
