//! The settings panel and its Advanced section.

use super::*;

impl App {
    pub(crate) fn settings_ui(&mut self, ui: &mut egui::Ui) {
        theme::card(ui, |ui| self.settings_grid(ui));
        ui.add_space(2.0);
        self.advanced_section(ui);
    }

    pub(crate) fn settings_grid(&mut self, ui: &mut egui::Ui) {
        egui::Grid::new("settings")
            .num_columns(2)
            .spacing([16.0, 10.0])
            .show(ui, |ui| {
                theme::row_label(ui, "Create");
                theme::segmented(
                    ui,
                    &mut self.settings.mode,
                    &[
                        (
                            Mode::Both,
                            "HDR + SDR",
                            "An HDR image and a tone-mapped SDR image for each image",
                        ),
                        (
                            Mode::Hdr,
                            "HDR only",
                            "Only the HDR image, in the format chosen below",
                        ),
                        (
                            Mode::Sdr,
                            "SDR only",
                            "Tone-mapped 8-bit image that looks right everywhere",
                        ),
                    ],
                );
                ui.end_row();

                theme::row_label(ui, "HDR format");
                ui.add_enabled_ui(self.settings.mode != Mode::Sdr, |ui| {
                    theme::segmented(
                        ui,
                        &mut self.settings.hdr_format,
                        &[
                            (
                                Format::Png,
                                "PNG",
                                "16-bit lossless PNG, shown in HDR by Windows Photos, Chrome \
                                 and Edge. The largest files.",
                            ),
                            (
                                Format::Avif,
                                "AVIF",
                                "10-bit AVIF, about a twentieth of the PNG's size, shown in HDR \
                                 by Chrome, Edge and Firefox. Takes a few seconds per image.",
                            ),
                            (
                                Format::UltraHdr,
                                "Ultra HDR JPEG",
                                "A JPEG that every app shows in SDR and that Chrome, Edge \
                                 and Android show in HDR.",
                            ),
                        ],
                    );
                });
                ui.end_row();

                ui.label("");
                let png =
                    self.settings.mode != Mode::Sdr && self.settings.hdr_format == Format::Png;
                ui.horizontal(|ui| {
                    ui.add_enabled(
                        png,
                        egui::Checkbox::new(
                            &mut self.settings.discord_png,
                            "Discord-compatible PNG",
                        ),
                    );
                    // Explains the option even while it doesn't apply.
                    theme::info_icon(ui).on_hover_text(
                        "Saves HDR PNGs without their cICP and mDCV chunks. Discord turns \
                         PNGs with cICP yellow; without it, Discord shows them in HDR with \
                         the right colors.\n\n\
                         Windows Photos and most other HDR viewers need cICP to show HDR, \
                         so they show these PNGs in SDR only. Leave this off unless you \
                         upload the saved files to Discord yourself: Copy for Discord \
                         already copies a Discord-compatible version.",
                    );
                });
                ui.end_row();

                theme::row_label(ui, "SDR format");
                ui.add_enabled_ui(self.settings.mode != Mode::Hdr, |ui| {
                    theme::segmented(
                        ui,
                        &mut self.settings.sdr_format,
                        &[
                            (
                                SdrKind::Png,
                                "PNG",
                                "8-bit lossless PNG, the exact tone-mapped picture",
                            ),
                            (
                                SdrKind::Jpeg,
                                "JPEG",
                                "JPEG at quality 90, a fraction of the PNG's size, for chats \
                                 and forums that don't show HDR. Transparency is lost.",
                            ),
                        ],
                    );
                });
                ui.end_row();

                theme::row_label(ui, "Save to");
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
                            let mut dialog = rfd::FileDialog::new().set_title("Save images to");
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

                theme::row_label(ui, "Resolution");
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

                theme::row_label(ui, "Clipboard");
                ui.checkbox(
                    &mut self.settings.copy_to_clipboard,
                    "Copy the last converted image",
                )
                .on_hover_text(
                    "After converting, copy the last image to the clipboard as a file \
                     (the HDR one if made), ready to paste into Discord, a browser or a folder",
                );
                ui.end_row();

                theme::row_label(ui, "Originals");
                ui.checkbox(
                    &mut self.settings.trash_originals,
                    format!("Move to the {RECYCLE_BIN} after converting"),
                )
                .on_hover_text(format!(
                    "Once all of an image's files are saved, move the original to the \
                     {RECYCLE_BIN}, where it can be restored. Images that fail, or whose \
                     files already existed, are kept, and so is everything when you stop \
                     the batch."
                ));
                ui.end_row();
            });
    }

    pub(crate) fn advanced_section(&mut self, ui: &mut egui::Ui) {
        let custom = !self.tuning.is_default();
        let title = if custom {
            "Advanced (custom)"
        } else {
            "Advanced"
        };
        egui::CollapsingHeader::new(title)
            .id_salt("advanced")
            .show(ui, |ui| {
                // Scrolls only when the window is too short for it, so
                // opening it never squeezes the file list away.
                let room = ui.ctx().content_rect().height() - MIN_LIST_AREA - self.bottom_rest;
                let shown = egui::ScrollArea::vertical()
                    .max_height(room.max(MIN_ADVANCED_HEIGHT))
                    .auto_shrink([false, true])
                    .show(ui, |ui| self.advanced_ui(ui));
                self.advanced_height = shown.inner_rect.height();
            });
    }

    pub(crate) fn advanced_ui(&mut self, ui: &mut egui::Ui) {
        self.watch_ui(ui);
        ui.checkbox(&mut self.settings.keep_date, "Keep the capture date")
            .on_hover_text(
                "Give each PNG the date of its screenshot, so folders sorted \
                         by date stay in capture order",
            );
        let s = &mut self.settings;
        ui.checkbox(&mut s.subfolders, "Save HDR and SDR images in subfolders")
            .on_hover_text("Put them in folders named hdr and sdr");
        let needed = s.suffix_needed();
        let mut suffix = s.type_suffix || needed;
        ui.add_enabled_ui(!needed, |ui| {
            if ui
                .checkbox(&mut suffix, "End file names in _hdr and _sdr")
                .on_hover_text("Off: shot.jxr becomes shot.png")
                .on_disabled_hover_text(
                    "Needed to tell the two PNGs apart when both are saved in the \
                             same folder",
                )
                .changed()
            {
                s.type_suffix = suffix;
            }
        });
        self.tuning_ui(ui);
    }

    pub(crate) fn tuning_ui(&mut self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new(
                "Not needed normally: by default the results match the original \
                 image. Use these only to fine-tune the result.",
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
                            "Roll off highlights brighter than this in the HDR image. \
                             Off keeps the original brightness.",
                        );
                });
                ui.add_enabled(
                    mode != Mode::Sdr && t.limit_hdr_peak,
                    nits(&mut t.hdr_peak, 100.0),
                );
                ui.end_row();

                ui.label("SDR white").on_hover_text(
                    "HDR brightness that becomes white in the SDR image. \
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

                ui.checkbox(&mut t.fixed_input_white, "Render white")
                    .on_hover_text(
                        "For OpenEXR, Radiance and float JPEG XL files: the \
                     brightness of a value of 1.0. Off uses the file's own value, or 80 nits.",
                    );
                ui.add_enabled(t.fixed_input_white, nits(&mut t.input_white, 1.0));
                ui.end_row();
            });
        if !t.is_default() && ui.button("Back to automatic").clicked() {
            *t = Tuning::default();
        }
    }
}

/// What the user picked last time. Tuning options are deliberately not
/// remembered: every session starts out matching the original screenshots.
#[derive(Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Settings {
    pub(crate) mode: Mode,
    pub(crate) hdr_format: Format,
    pub(crate) sdr_format: SdrKind,
    pub(crate) save_to_folder: bool,
    pub(crate) output_dir: Option<PathBuf>,
    pub(crate) copy_to_clipboard: bool,
    pub(crate) keep_date: bool,
    /// Put HDR and SDR PNGs into `hdr` and `sdr` subfolders.
    pub(crate) subfolders: bool,
    /// End file names in `_hdr` or `_sdr`. Always on when both types share
    /// a folder.
    pub(crate) type_suffix: bool,
    /// Write HDR PNGs without `cICP`, so Discord shows them correctly.
    pub(crate) discord_png: bool,
    /// Move each original to the Recycle Bin once its outputs are written.
    pub(crate) trash_originals: bool,
    /// Convert new images saved to `watch_dir`.
    pub(crate) watch: bool,
    pub(crate) watch_dir: Option<PathBuf>,
}

impl Settings {
    /// Whether the names need `_hdr`/`_sdr` to tell the two PNGs apart.
    pub(crate) fn suffix_needed(&self) -> bool {
        self.mode == Mode::Both && !self.subfolders
    }

    pub(crate) fn naming(&self) -> Naming {
        Naming {
            pattern: if self.type_suffix || self.suffix_needed() {
                Naming::DEFAULT_PATTERN.into()
            } else {
                "{name}".into()
            },
            subfolders: self.subfolders,
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mode: Mode::Both,
            hdr_format: Format::Png,
            sdr_format: SdrKind::Png,
            save_to_folder: false,
            output_dir: None,
            copy_to_clipboard: true,
            keep_date: true,
            subfolders: false,
            type_suffix: true,
            discord_png: false,
            trash_originals: false,
            watch: false,
            watch_dir: watching::default_watch_dir(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Mode {
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

/// File format of the HDR output.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Format {
    Png,
    Avif,
    UltraHdr,
}

impl From<Format> for HdrFormat {
    fn from(format: Format) -> Self {
        match format {
            Format::Png => Self::Png,
            Format::Avif => Self::Avif,
            Format::UltraHdr => Self::UltraHdr,
        }
    }
}

/// File format of the SDR output.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum SdrKind {
    Png,
    Jpeg,
}

impl From<SdrKind> for SdrFormat {
    fn from(kind: SdrKind) -> Self {
        match kind {
            SdrKind::Png => Self::Png,
            SdrKind::Jpeg => Self::Jpeg {
                quality: Self::DEFAULT_JPEG_QUALITY,
            },
        }
    }
}

/// Optional tuning, off by default.
pub(crate) struct Tuning {
    pub(crate) limit_hdr_peak: bool,
    pub(crate) hdr_peak: f32,
    pub(crate) sdr_white: f32,
    pub(crate) fixed_sdr_peak: bool,
    pub(crate) sdr_peak: f32,
    pub(crate) fixed_input_white: bool,
    pub(crate) input_white: f32,
}

impl Default for Tuning {
    fn default() -> Self {
        Self {
            limit_hdr_peak: false,
            hdr_peak: 1000.0,
            sdr_white: SdrOptions::default().white_nits,
            fixed_sdr_peak: false,
            sdr_peak: 1000.0,
            fixed_input_white: false,
            input_white: 80.0,
        }
    }
}

impl Tuning {
    pub(crate) fn is_default(&self) -> bool {
        !self.limit_hdr_peak
            && !self.fixed_sdr_peak
            && !self.fixed_input_white
            && self.sdr_white == SdrOptions::default().white_nits
    }
}

/// What size the listed files come out at with a given height limit.
#[derive(Debug, PartialEq)]
pub(crate) enum OutputSize {
    /// No file in the list, or none whose size could be read.
    Unknown,
    Same(u32, u32),
    Varies,
}

pub(crate) fn output_size(files: &[Entry], max_height: Option<u32>) -> OutputSize {
    let mut sizes = files
        .iter()
        .filter_map(|e| e.size)
        .map(|(w, h)| max_height.map_or((w, h), |max| hdrshot_core::resize::fit_height(w, h, max)));
    let Some(first) = sizes.next() else {
        return OutputSize::Unknown;
    };
    if sizes.all(|s| s == first) {
        OutputSize::Same(first.0, first.1)
    } else {
        OutputSize::Varies
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(size: Option<(u32, u32)>) -> Entry {
        Entry {
            path: PathBuf::new(),
            status: Status::Waiting,
            size,
            thumb: Thumb::Failed,
        }
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
