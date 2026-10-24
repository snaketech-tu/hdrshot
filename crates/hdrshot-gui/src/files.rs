//! The list of files, with their rows and menus.

use super::*;

impl App {
    pub(crate) fn failed(&self) -> Vec<usize> {
        (0..self.files.len())
            .filter(|&i| matches!(self.files[i].status, Status::Failed(_)))
            .collect()
    }

    /// Takes `rows` out of the list, keeping them for a few seconds so
    /// Undo can bring them back.
    pub(crate) fn remove(&mut self, mut rows: Vec<usize>, ctx: &egui::Context) {
        rows.sort_unstable();
        rows.dedup();
        if rows.is_empty() {
            return;
        }
        let mut removed: Vec<(usize, Entry)> = rows
            .iter()
            .rev()
            .map(|&i| (i, self.files.remove(i)))
            .collect();
        removed.reverse();
        self.selection.after_removal(&rows);
        self.removed = Some(Removed {
            rows: removed,
            until: ctx.input(|i| i.time) + UNDO_SECONDS,
        });
        self.notice = None;
    }

    /// Puts the rows removed last back where they were.
    pub(crate) fn undo_remove(&mut self) {
        let Some(removed) = self.removed.take() else {
            return;
        };
        let mut restored = Vec::new();
        // In order, so each goes back to its old place.
        for (i, entry) in removed.rows {
            if self.files.iter().any(|e| e.path == entry.path) {
                continue;
            }
            let at = i.min(self.files.len());
            self.files.insert(at, entry);
            restored.push(at);
        }
        self.selection.set_all(restored);
    }

    /// Removes the files that converted, keeping the rest to retry or add to.
    pub(crate) fn clear_finished(&mut self, ctx: &egui::Context) {
        let done = (0..self.files.len())
            .filter(|&i| matches!(self.files[i].status, Status::Done(_)))
            .collect();
        self.remove(done, ctx);
        self.banner = false;
    }

    /// Empties the list.
    pub(crate) fn clear_all(&mut self, ctx: &egui::Context) {
        self.remove((0..self.files.len()).collect(), ctx);
        self.batch = Batch::default();
        self.banner = false;
    }

    /// Rows the selected row's actions apply to: all selected ones if `row`
    /// is one of them, else `row` alone.
    fn picked(&self, row: usize) -> Vec<usize> {
        if self.selection.contains(row) {
            self.selection.rows().to_vec()
        } else {
            vec![row]
        }
    }

    /// Copies row `i`'s image for Discord, in the background.
    pub(crate) fn copy_for_discord(&mut self, i: usize, ctx: &egui::Context) {
        let entry = &self.files[i];
        let Status::Done(outputs) = &entry.status else {
            self.show_toast("Convert it first to copy it".into(), ctx);
            return;
        };
        if outputs.hdr.is_none() && outputs.sdr.is_none() {
            return;
        }
        let (input, outputs) = (entry.path.clone(), outputs.clone());
        if convert::copy_makes_png(&outputs) {
            self.show_toast("Making the PNG for Discord…".into(), ctx);
        }
        let clipboard = self.clipboard.clone();
        let options = self.convert_options(Existing::Skip);
        convert::in_background(self.copied.0.clone(), ctx.clone(), move || {
            convert::copy(&clipboard, &input, &outputs, &options)
        });
    }

    /// The keyboard shortcuts of the list, not while a text field has
    /// focus or a menu or dialog is open:
    /// - Delete removes the selected rows and Ctrl+Z (Cmd+Z on macOS)
    ///   brings them back; neither while a batch runs.
    /// - Enter converts the files not converted yet.
    /// - The arrow keys move the selection, Ctrl+A selects every row and
    ///   Escape none.
    /// - Space shows the large preview of the selected row.
    /// - Ctrl+C copies the selected row's image for Discord, Ctrl+O adds
    ///   files.
    ///
    /// On the Settings page, Escape goes back to the files.
    pub(crate) fn shortcuts(&mut self, ctx: &egui::Context) {
        // A menu or dialog gets the keys, so Escape closes it.
        let dialog = matches!(
            self.update,
            Update::Available { .. } | Update::Installing(_) | Update::Opened
        );
        if self.asking.is_some()
            || dialog
            || egui::Popup::is_any_open(ctx)
            || ctx.memory(|m| m.focused().is_some())
        {
            return;
        }
        let pressed = |key| ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, key));
        let command = |key| ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, key));
        if self.settings_open {
            if pressed(egui::Key::Escape) {
                self.settings_open = false;
            }
            return;
        }
        if self.viewer.is_some() {
            self.viewer_shortcuts(ctx);
            return;
        }
        if command(egui::Key::O) {
            self.choose_files();
        }
        if command(egui::Key::A) {
            self.selection.all(self.files.len());
        }
        // egui turns Ctrl+C into a copy event rather than a key press.
        let copy = ctx.input_mut(|i| {
            let before = i.events.len();
            i.events.retain(|e| !matches!(e, egui::Event::Copy));
            i.events.len() < before
        });
        if (copy || command(egui::Key::C))
            && let Some(row) = self.selection.anchor()
        {
            self.copy_for_discord(row, ctx);
        }
        if pressed(egui::Key::Space)
            && let Some(row) = self.selection.anchor()
        {
            self.open_viewer(row, ctx);
        }
        if pressed(egui::Key::ArrowDown) {
            self.selection.step(true, self.files.len());
        }
        if pressed(egui::Key::ArrowUp) {
            self.selection.step(false, self.files.len());
        }
        if pressed(egui::Key::Escape) {
            self.selection.clear();
        }
        if self.running() {
            return;
        }
        if command(egui::Key::Z) {
            self.undo_remove();
        }
        if pressed(egui::Key::Delete) || pressed(egui::Key::Backspace) {
            let rows = self.selection.rows().to_vec();
            self.remove(rows, ctx);
        }
        if pressed(egui::Key::Enter) && self.can_convert() {
            let pending = self.pending();
            if !pending.is_empty() {
                self.start_some(pending, ctx);
            }
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

    pub(crate) fn file_list(&mut self, ui: &mut egui::Ui) {
        let running = self.running();
        let mut action = None;
        let mut hovered_thumb = None;
        let visuals = ui.visuals();
        let card = visuals.faint_bg_color;
        let selection = visuals.selection.bg_fill.gamma_multiply(0.25);
        let hover = visuals.widgets.inactive.weak_bg_fill;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                for (i, entry) in self.files.iter().enumerate() {
                    let (rect, row) = ui.allocate_exact_size(
                        Vec2::new(ui.available_width(), ROW_HEIGHT),
                        Sense::click(),
                    );
                    if !ui.is_rect_visible(rect) {
                        continue;
                    }
                    // The row's buttons take the hover from the row itself.
                    let pointed = ui.rect_contains_pointer(rect);
                    let selected = self.selection.contains(i);
                    let fill = if selected {
                        selection
                    } else if pointed {
                        hover
                    } else {
                        card
                    };
                    ui.painter().rect_filled(rect, 9.0, fill);
                    let outputs = match &entry.status {
                        Status::Done(outputs) if outputs.hdr.is_some() || outputs.sdr.is_some() => {
                            Some(outputs)
                        }
                        _ => None,
                    };
                    let primary = outputs.and_then(|o| o.hdr.as_ref().or(o.sdr.as_ref()));
                    if row.clicked() {
                        let modifiers = ui.input(|i| i.modifiers);
                        action = Some(RowAction::Click(i, modifiers));
                    }
                    if row.secondary_clicked() && !selected {
                        action = Some(RowAction::Click(i, egui::Modifiers::NONE));
                    }
                    if row.double_clicked()
                        && let Some(path) = primary
                    {
                        action = Some(RowAction::Open(path.clone()));
                    }
                    row.context_menu(|ui| {
                        let picked = self.picked(i);
                        if let Some(menu) = row_menu(ui, i, entry, &picked, running) {
                            action = Some(menu);
                        }
                    });

                    let inner = egui::UiBuilder::new()
                        .max_rect(rect.shrink2(Vec2::new(8.0, 0.0)))
                        .layout(egui::Layout::right_to_left(egui::Align::Center));
                    ui.scope_builder(inner, |ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        ui.add_space(4.0);
                        status_icon(ui, &entry.status);
                        ui.add_space(4.0);
                        // Buttons for the row pointed at, or picked alone.
                        let full = pointed || (selected && self.selection.len() == 1);
                        if full
                            && !running
                            && theme::small_icon_button(ui, icons::cross)
                                .on_hover_text("Remove from the list")
                                .clicked()
                        {
                            action = Some(RowAction::Remove(vec![i]));
                        }
                        // The output once there is one, else the original.
                        let shown = primary.unwrap_or(&entry.path);
                        if full
                            && theme::small_icon_button(ui, icons::folder)
                                .on_hover_text("Show in folder")
                                .clicked()
                        {
                            action = Some(RowAction::ShowInFolder(shown.clone()));
                        }
                        if full
                            && let Some(path) = primary
                            && theme::small_icon_button(ui, icons::open)
                                .on_hover_text(format!(
                                    "Open {}",
                                    path.file_name().unwrap_or_default().to_string_lossy()
                                ))
                                .clicked()
                        {
                            action = Some(RowAction::Open(path.clone()));
                        }
                        // The newest result keeps its button, ready to share.
                        let latest = self.last_converted.as_ref() == Some(&entry.path);
                        if outputs.is_some() && (full || latest) {
                            ui.add_space(4.0);
                            if ui
                                .small_button("Copy for Discord")
                                .on_hover_text(DISCORD_COPY_HINT)
                                .clicked()
                            {
                                action = Some(RowAction::Copy(i));
                            }
                        }
                        if matches!(entry.status, Status::Failed(_)) && !running {
                            ui.add_space(4.0);
                            if ui
                                .small_button("Retry")
                                .on_hover_text("Convert it again")
                                .clicked()
                            {
                                action = Some(RowAction::Convert(vec![i]));
                            }
                        }
                        ui.add_space(8.0);
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.add_space(2.0);
                            let thumb = thumb_ui(
                                ui,
                                &entry.thumb,
                                self.shown.as_ref().filter(|s| s.path == entry.path),
                            );
                            if thumb.hovered() {
                                hovered_thumb = Some(i);
                            }
                            if thumb.clicked() {
                                action = Some(RowAction::View(i));
                            }
                            ui.add_space(8.0);
                            // Sized to the two lines, so the row centers them.
                            let (name_font, detail_font) = (
                                egui::FontId::proportional(13.5),
                                egui::FontId::proportional(12.0),
                            );
                            let height = ui.ctx().fonts_mut(|f| {
                                f.row_height(&name_font) + f.row_height(&detail_font)
                            }) + 3.0;
                            let size = Vec2::new(ui.available_width(), height);
                            let layout = egui::Layout::top_down(egui::Align::Min);
                            ui.allocate_ui_with_layout(size, layout, |ui| {
                                ui.spacing_mut().item_spacing.y = 3.0;
                                let name =
                                    entry.path.file_name().unwrap_or_default().to_string_lossy();
                                ui.add(
                                    egui::Label::new(RichText::new(name.as_ref()).font(name_font))
                                        .truncate()
                                        .selectable(false),
                                )
                                .on_hover_text(entry.path.display().to_string());
                                let (detail, color) = detail(ui, entry);
                                let label = ui.add(
                                    egui::Label::new(
                                        RichText::new(&detail).font(detail_font).color(color),
                                    )
                                    .truncate()
                                    .selectable(false),
                                );
                                if matches!(entry.status, Status::Failed(_)) {
                                    label.on_hover_text(detail);
                                }
                            });
                        });
                    });
                }
            });
        let ctx = ui.ctx().clone();
        self.update_shown(hovered_thumb, &ctx);
        match action {
            None => {}
            Some(RowAction::Click(i, modifiers)) => {
                if modifiers.shift {
                    self.selection.extend_to(i);
                } else if modifiers.command {
                    self.selection.toggle(i);
                } else {
                    self.selection.set(i);
                }
            }
            Some(RowAction::View(i)) => self.open_viewer(i, &ctx),
            Some(RowAction::Open(path)) => open(&path),
            Some(RowAction::ShowInFolder(path)) => show_in_folder(&path),
            Some(RowAction::Copy(i)) => self.copy_for_discord(i, &ctx),
            Some(RowAction::Convert(rows)) => self.start_some(rows, &ctx),
            Some(RowAction::Remove(rows)) => self.remove(rows, &ctx),
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
    /// What the row says about the original, e.g. "3840 × 2160 · JXR ·
    /// 24.1 MB".
    pub(crate) info: String,
    /// What the row says once converted, e.g. "HDR PNG 10.2 MB · SDR PNG
    /// 2.1 MB".
    pub(crate) saved: String,
    pub(crate) thumb: Thumb,
}

/// Height of a row in the list.
const ROW_HEIGHT: f32 = 58.0;

/// The second line of a row and its color.
fn detail(ui: &egui::Ui, entry: &Entry) -> (String, Color32) {
    let weak = ui.visuals().weak_text_color();
    match &entry.status {
        Status::Waiting => (entry.info.clone(), weak),
        Status::Converting => ("Converting…".to_owned(), theme::accent(ui)),
        Status::Done(_) => (entry.saved.clone(), weak),
        Status::Failed(error) => (format!("Failed: {error}"), ui.visuals().error_fg_color),
    }
}

/// What a row says about an original: its size in pixels, type and size
/// on disk, as far as known.
pub(crate) fn input_info(path: &Path, size: Option<(u32, u32)>, bytes: Option<u64>) -> String {
    let mut parts = Vec::new();
    if let Some((w, h)) = size {
        parts.push(format!("{w} × {h}"));
    }
    if let Some(ext) = path.extension() {
        parts.push(ext.to_string_lossy().to_uppercase());
    }
    if let Some(bytes) = bytes {
        parts.push(hdrshot_core::share::megabytes(bytes));
    }
    parts.join(" · ")
}

/// What a row says once its original is converted, and the size of the
/// files written.
pub(crate) fn saved_summary(outputs: &Outputs) -> (String, u64) {
    let mut parts = Vec::new();
    let mut total = 0;
    for (path, hdr) in [(&outputs.hdr, true), (&outputs.sdr, false)] {
        let Some(path) = path else { continue };
        let kind = output_kind(path, hdr);
        match std::fs::metadata(path) {
            Ok(meta) => {
                total += meta.len();
                parts.push(format!(
                    "{kind} {}",
                    hdrshot_core::share::megabytes(meta.len())
                ));
            }
            Err(_) => parts.push(kind),
        }
    }
    let kept = outputs.skipped.len();
    if parts.is_empty() {
        parts.push("Already there, left alone".to_owned());
    } else if kept > 0 {
        parts.push(format!(
            "{kept} existing file{} kept",
            if kept == 1 { "" } else { "s" }
        ));
    }
    if outputs.sdr_input {
        parts.push("the original is SDR".to_owned());
    }
    (parts.join(" · "), total)
}

/// "HDR PNG", "Ultra HDR JPEG", "SDR JPEG" and so on, from the file's
/// extension.
fn output_kind(path: &Path, hdr: bool) -> String {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match (hdr, ext.as_str()) {
        (true, "jpg" | "jpeg") => "Ultra HDR JPEG".to_owned(),
        (false, "jpg" | "jpeg") => "SDR JPEG".to_owned(),
        (true, ext) => format!("HDR {}", ext.to_uppercase()),
        (false, ext) => format!("SDR {}", ext.to_uppercase()),
    }
}

/// The large preview of the row under the mouse, tone-mapped automatically
/// and, if the fine-tuning settings are changed, with them.
pub(crate) struct Shown {
    pub(crate) path: PathBuf,
    pub(crate) sdr: SdrOptions,
    pub(crate) auto: egui::TextureHandle,
    pub(crate) tuned: Option<egui::TextureHandle>,
}

/// Something done to rows of the file list.
pub(crate) enum RowAction {
    /// A click on a row, with the keys held down.
    Click(usize, egui::Modifiers),
    /// Show the row's large preview.
    View(usize),
    Open(PathBuf),
    ShowInFolder(PathBuf),
    /// Copy the row's image for Discord.
    Copy(usize),
    Convert(Vec<usize>),
    Remove(Vec<usize>),
}

/// Rows just removed, kept for a few seconds so Undo can bring them back.
pub(crate) struct Removed {
    /// Each with the place it had, in order.
    pub(crate) rows: Vec<(usize, Entry)>,
    /// `egui` time at which Undo disappears.
    pub(crate) until: f64,
}

/// How long Undo shows after removing files.
pub(crate) const UNDO_SECONDS: f64 = 6.0;

/// The folder all of `dirs` are, if they are the same one.
pub(crate) fn common_folder<'a>(dirs: &[&'a Path]) -> Option<&'a Path> {
    let first = *dirs.first()?;
    dirs.iter().all(|&d| d == first).then_some(first)
}

/// The thumbnail at the start of a row, filling its box and cropped to it,
/// with the preview `shown` (if it is this row's) when hovered. Clicking
/// it opens the large preview.
pub(crate) fn thumb_ui(ui: &mut egui::Ui, thumb: &Thumb, shown: Option<&Shown>) -> egui::Response {
    let box_size = Vec2::new(THUMB_SIZE.0 as f32, THUMB_SIZE.1 as f32);
    let radius = 5.0;
    let (rect, response) = ui.allocate_exact_size(box_size, Sense::click());
    let Thumb::Ready { texture, .. } = thumb else {
        let visuals = ui.visuals();
        ui.painter()
            .rect_filled(rect, radius, visuals.widgets.inactive.bg_fill);
        if matches!(thumb, Thumb::Failed) {
            let icon = egui::Rect::from_center_size(rect.center(), Vec2::splat(18.0));
            icons::image(ui.painter(), icon, visuals.weak_text_color());
        }
        return response;
    };
    let [w, h] = texture.size();
    let uv = cover_uv(w as f32 / h.max(1) as f32, box_size.x / box_size.y);
    egui::Image::new((texture.id(), box_size))
        .uv(uv)
        .corner_radius(radius)
        .paint_at(ui, rect);
    let response = response.on_hover_cursor(egui::CursorIcon::ZoomIn);
    if let Some(shown) = shown {
        response.clone().on_hover_ui(|ui| {
            let show = |ui: &mut egui::Ui, texture: &egui::TextureHandle| {
                let [w, h] = texture.size();
                ui.image((texture.id(), Vec2::new(w as f32, h as f32)));
            };
            match &shown.tuned {
                Some(tuned) => {
                    ui.label("Automatic");
                    show(ui, &shown.auto);
                    ui.label("With your fine-tuning");
                    show(ui, tuned);
                }
                None => show(ui, &shown.auto),
            }
            ui.weak("SDR preview; the HDR image keeps the full brightness.");
            ui.weak("Click for a larger one.");
        });
    }
    response
}

/// The part of an image with aspect ratio `image` that fills a box with
/// aspect ratio `fill`, centered, in texture coordinates.
fn cover_uv(image: f32, fill: f32) -> egui::Rect {
    if image > fill {
        let crop = (1.0 - fill / image) / 2.0;
        egui::Rect::from_min_max(egui::pos2(crop, 0.0), egui::pos2(1.0 - crop, 1.0))
    } else {
        let crop = (1.0 - image / fill) / 2.0;
        egui::Rect::from_min_max(egui::pos2(0.0, crop), egui::pos2(1.0, 1.0 - crop))
    }
}

/// The right-click menu of row `i`, acting on the `picked` rows when
/// several are selected.
pub(crate) fn row_menu(
    ui: &mut egui::Ui,
    i: usize,
    entry: &Entry,
    picked: &[usize],
    running: bool,
) -> Option<RowAction> {
    let mut action = None;
    if picked.len() > 1 {
        if !running
            && ui
                .button(format!("Convert {}", images(picked.len())))
                .clicked()
        {
            action = Some(RowAction::Convert(picked.to_vec()));
        }
        if !running
            && ui
                .button(format!("Remove {} from list", images(picked.len())))
                .clicked()
        {
            action = Some(RowAction::Remove(picked.to_vec()));
        }
        if action.is_some() {
            ui.close();
        }
        return action;
    }
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
    if outputs.is_some()
        && ui
            .button("Copy for Discord")
            .on_hover_text(DISCORD_COPY_HINT)
            .clicked()
    {
        action = Some(RowAction::Copy(i));
    }
    if matches!(entry.thumb, Thumb::Ready { .. }) && ui.button("Preview").clicked() {
        action = Some(RowAction::View(i));
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
            action = Some(RowAction::Convert(vec![i]));
        }
        if ui.button("Remove from list").clicked() {
            action = Some(RowAction::Remove(vec![i]));
        }
    }
    if action.is_some() {
        ui.close();
    }
    action
}

/// The drawn icon at the end of a row: a ring of dots while waiting, a
/// spinner while converting, a check once done and a "!" if it failed.
/// Hovering it lists the files saved, or the error.
pub(crate) fn status_icon(ui: &mut egui::Ui, status: &Status) {
    let size = Vec2::splat(20.0);
    let radius = 9.0;
    if let Status::Converting = status {
        ui.add_sized(
            size,
            egui::Spinner::new().size(16.0).color(theme::accent(ui)),
        );
        return;
    }
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    let center = rect.center();
    let painter = ui.painter();
    let weak = ui.visuals().weak_text_color();
    match status {
        Status::Converting => {}
        Status::Waiting => {
            icons::waiting(painter, center, radius * 0.8, weak);
            response.on_hover_text("Waiting to be converted");
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
                icons::skipped(painter, center, radius, weak);
            } else {
                icons::done(painter, center, radius, ok_color(ui));
            }
            response.on_hover_text(tooltip);
        }
        Status::Failed(error) => {
            icons::failed(painter, center, radius, ui.visuals().error_fg_color);
            response.on_hover_text(error);
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
    fn thumbnails_are_cropped_to_fill() {
        let wide = cover_uv(2.0, 1.0);
        assert_eq!(wide.min, egui::pos2(0.25, 0.0));
        assert_eq!(wide.max, egui::pos2(0.75, 1.0));
        let tall = cover_uv(0.5, 1.0);
        assert_eq!(tall.min, egui::pos2(0.0, 0.25));
        let same = cover_uv(16.0 / 9.0, 16.0 / 9.0);
        assert_eq!(
            same,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0))
        );
    }

    #[test]
    fn rows_describe_their_files() {
        let path = Path::new("shot.jxr");
        assert_eq!(
            input_info(path, Some((3840, 2160)), Some(24_100_000)),
            "3840 × 2160 · JXR · 24.1 MB"
        );
        assert_eq!(input_info(path, None, None), "JXR");
        assert_eq!(output_kind(Path::new("a.jpg"), true), "Ultra HDR JPEG");
        assert_eq!(output_kind(Path::new("a.avif"), true), "HDR AVIF");
        assert_eq!(output_kind(Path::new("a.jpg"), false), "SDR JPEG");
        let skipped = Outputs {
            skipped: vec![PathBuf::from("shot_hdr.png")],
            ..Default::default()
        };
        assert_eq!(
            saved_summary(&skipped),
            ("Already there, left alone".into(), 0)
        );
    }
}
