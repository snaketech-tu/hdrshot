//! The settings: chips under the file list that sum them up, each with a
//! menu for its choices, and the Settings page holding all of them.

use super::*;

/// How many folders saved into before the save menu offers.
const RECENT_DIRS: usize = 3;

/// Height limits offered for the output, `None` keeping the source size.
const HEIGHTS: [Option<u32>; 5] = [None, Some(2160), Some(1440), Some(1080), Some(720)];

impl App {
    /// The row of chips under the file list.
    pub(crate) fn summary_chips(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(5.0, 6.0);
            let output = self.settings.output_summary();
            chip_menu(ui, "output", &output, false, "What to create", |ui| {
                settings_grid(ui, "output menu", |ui| self.output_rows(ui));
            });
            let save = self.settings.save_summary();
            let no_folder = self.settings.save_to_folder && self.settings.output_dir.is_none();
            chip_menu(ui, "save", &save, no_folder, "Where to save", |ui| {
                if self.save_menu(ui) {
                    ui.close();
                }
            });
            let size = size_summary(self.max_height);
            chip_menu(ui, "size", &size, false, "Largest output size", |ui| {
                ui.set_max_width(260.0);
                let labels: Vec<_> = HEIGHTS
                    .iter()
                    .map(|&h| (h, self.resolution_label(h)))
                    .collect();
                for (h, label) in labels {
                    if ui
                        .selectable_value(&mut self.max_height, h, label)
                        .clicked()
                    {
                        ui.close();
                    }
                }
                ui.label(
                    RichText::new("Keeps the aspect ratio. Smaller images are never scaled up.")
                        .weak()
                        .size(12.0),
                );
            });
            let after = self.settings.after_summary();
            chip_menu(
                ui,
                "after",
                &after,
                false,
                "What happens after converting",
                |ui| {
                    settings_grid(ui, "after menu", |ui| self.after_rows(ui));
                },
            );
            if !self.tuning.is_default()
                && theme::chip(ui, "Custom tuning", false, true)
                    .on_hover_text(
                        "Fine-tuning is changed for this session. Click to see it in \
                         Settings.",
                    )
                    .clicked()
            {
                self.settings_open = true;
            }
        });
    }

    /// The Settings page, in place of the file list.
    pub(crate) fn settings_page(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if theme::icon_button(ui, icons::back)
                .on_hover_text("Back to the files (Esc)")
                .clicked()
            {
                self.settings_open = false;
            }
            ui.add_space(2.0);
            ui.label(RichText::new("Settings").font(theme::semibold(18.0)));
        });
        ui.add_space(4.0);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if self.running() {
                    ui.weak("Settings can be changed once the images are converted.");
                }
                theme::section(ui, "General");
                theme::card(ui, |ui| {
                    settings_grid(ui, "general", |ui| self.general_rows(ui));
                });
                ui.add_enabled_ui(!self.running(), |ui| {
                    theme::section(ui, "Output");
                    theme::card(ui, |ui| {
                        settings_grid(ui, "output", |ui| {
                            self.output_rows(ui);
                            self.resolution_row(ui);
                        });
                    });
                    theme::section(ui, "Saving");
                    theme::card(ui, |ui| self.saving_ui(ui));
                    theme::section(ui, "After converting");
                    theme::card(ui, |ui| {
                        settings_grid(ui, "after", |ui| self.after_rows(ui))
                    });
                    theme::section(ui, "Watch folder");
                    theme::card(ui, |ui| self.watch_ui(ui));
                    theme::section(ui, "Fine-tuning");
                    theme::card(ui, |ui| self.tuning_ui(ui));
                });
            });
    }

    /// The theme, the weekly update check and the default app: rows of a
    /// two-column grid.
    fn general_rows(&mut self, ui: &mut egui::Ui) {
        theme::row_label(ui, "Theme");
        if theme::segmented(
            ui,
            &mut self.settings.theme,
            &[
                (
                    ThemeChoice::System,
                    "System",
                    "Light or dark, as the system is set",
                ),
                (ThemeChoice::Light, "Light", "Always light"),
                (ThemeChoice::Dark, "Dark", "Always dark"),
            ],
        ) {
            self.settings.theme.apply(ui.ctx());
        }
        ui.end_row();

        theme::row_label(ui, "Updates");
        ui.checkbox(
            &mut self.settings.update_check,
            "Check for updates once a week",
        )
        .on_hover_text(
            "When hdrshot starts, ask GitHub for its latest release at most once \
             a week. If there is a newer one, the link at the bottom of the window \
             offers it; nothing is downloaded until you choose Update.",
        );
        ui.end_row();

        if assoc::SUPPORTED {
            theme::row_label(ui, "Files");
            self.default_app_ui(ui);
            ui.end_row();
        }
    }

    /// What to create, in which formats: rows of a two-column grid.
    pub(crate) fn output_rows(&mut self, ui: &mut egui::Ui) {
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
        let png = self.settings.mode != Mode::Sdr && self.settings.hdr_format == Format::Png;
        ui.horizontal(|ui| {
            ui.add_enabled(
                png,
                egui::Checkbox::new(&mut self.settings.discord_png, "Discord-compatible PNG"),
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
    }

    /// The Resolution row of the Settings page's grid.
    pub(crate) fn resolution_row(&mut self, ui: &mut egui::Ui) {
        theme::row_label(ui, "Resolution");
        let selected = self.resolution_label(self.max_height);
        let labels: Vec<_> = HEIGHTS
            .iter()
            .map(|&h| (h, self.resolution_label(h)))
            .collect();
        egui::ComboBox::from_id_salt("resolution")
            .selected_text(selected)
            .show_ui(ui, |ui| {
                for (h, label) in labels {
                    ui.selectable_value(&mut self.max_height, h, label);
                }
            })
            .response
            .on_hover_text(RESOLUTION_HINT);
        ui.end_row();
    }

    /// A height limit's name, with the size the listed files come out at.
    pub(crate) fn resolution_label(&self, max_height: Option<u32>) -> String {
        let name = max_height.map_or_else(|| "Source".to_owned(), |h| format!("{h}p"));
        match output_size(&self.files, max_height) {
            OutputSize::Unknown => name,
            OutputSize::Same(w, h) => format!("{name} ({w}×{h})"),
            OutputSize::Varies => format!("{name} (sizes vary)"),
        }
    }

    /// The choices of where to save: next to each original, one of the
    /// folders chosen lately, or a new one. Returns whether one was made.
    pub(crate) fn save_menu(&mut self, ui: &mut egui::Ui) -> bool {
        let mut chosen = false;
        let folders = self.settings.save_folders();
        let s = &mut self.settings;
        if ui
            .selectable_label(!s.save_to_folder, "Next to each original")
            .on_hover_text("Save the new files in the folder of the image they come from")
            .clicked()
        {
            s.save_to_folder = false;
            chosen = true;
        }
        for dir in folders {
            let selected = s.save_to_folder && s.output_dir.as_ref() == Some(&dir);
            if ui
                .selectable_label(selected, folder_name(&dir))
                .on_hover_text(dir.display().to_string())
                .clicked()
            {
                s.use_output_dir(dir);
                chosen = true;
            }
        }
        ui.separator();
        if ui.button("Choose a folder…").clicked() {
            let mut dialog = rfd::FileDialog::new().set_title("Save images to");
            if let Some(dir) = &s.output_dir {
                dialog = dialog.set_directory(dir);
            }
            if let Some(dir) = dialog.pick_folder() {
                s.use_output_dir(dir);
            }
            chosen = true;
        }
        chosen
    }

    /// Where the images go and how they are named.
    pub(crate) fn saving_ui(&mut self, ui: &mut egui::Ui) {
        settings_grid(ui, "saving", |ui| {
            theme::row_label(ui, "Save to");
            let selected = match (&self.settings.output_dir, self.settings.save_to_folder) {
                (_, false) => "Next to each original".to_owned(),
                (Some(dir), true) => dir.display().to_string(),
                (None, true) => "Choose a folder…".to_owned(),
            };
            egui::ComboBox::from_id_salt("save to")
                .selected_text(selected)
                .width(ui.available_width().min(300.0))
                .truncate()
                .show_ui(ui, |ui| self.save_menu(ui));
            ui.end_row();
        });
        ui.add_space(4.0);
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
        self.name_preview_ui(ui);
    }

    /// The names of the files an image gets with the current settings,
    /// using the first listed image or a sample name.
    fn name_preview_ui(&self, ui: &mut egui::Ui) {
        let input = self
            .files
            .first()
            .map_or_else(|| PathBuf::from(SAMPLE_INPUT), |entry| entry.path.clone());
        let names = name_preview(&input, &self.convert_options(Existing::Skip));
        ui.add_space(6.0);
        // The page's color, set into the card.
        egui::Frame::new()
            .fill(ui.visuals().panel_fill)
            .corner_radius(6.0)
            .inner_margin(egui::Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 3.0;
                let source = input.file_name().unwrap_or_default().to_string_lossy();
                ui.label(RichText::new(format!("{source} becomes")).size(12.0).weak());
                for name in names {
                    ui.add(egui::Label::new(RichText::new(name).size(13.0)).truncate());
                }
            });
    }

    /// The clipboard copy and the Recycle Bin: rows of a two-column grid.
    pub(crate) fn after_rows(&mut self, ui: &mut egui::Ui) {
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
    }

    pub(crate) fn tuning_ui(&mut self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new(
                "Not needed normally: by default the results match the original \
                 image. Use these only to fine-tune the result. They reset when \
                 hdrshot closes.",
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

const RESOLUTION_HINT: &str = "Scale images taller than this down to this height, keeping the \
     aspect ratio, so ultrawide images stay ultrawide. Smaller images are never scaled up. \
     The size in brackets is what the listed files come out at.";

/// The image whose names the Settings page shows while the list is empty.
const SAMPLE_INPUT: &str = "Screenshot 2026-10-14 212314.jxr";

/// The files `input` becomes with `options`, as paths from the folder they
/// are saved in, e.g. `hdr/shot_hdr.png` with subfolders.
pub(crate) fn name_preview(input: &Path, options: &ConvertOptions) -> Vec<String> {
    let outputs = hdrshot_core::output_paths(input, options);
    let dir = options
        .output_dir
        .clone()
        .or_else(|| input.parent().map(Path::to_path_buf))
        .unwrap_or_default();
    [outputs.hdr, outputs.sdr]
        .into_iter()
        .flatten()
        .map(|path| {
            let path = path.strip_prefix(&dir).unwrap_or(&path);
            path.components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/")
        })
        .collect()
}

/// A two-column grid of labelled settings.
fn settings_grid(ui: &mut egui::Ui, id: &str, add_rows: impl FnOnce(&mut egui::Ui)) {
    egui::Grid::new(id)
        .num_columns(2)
        .spacing([16.0, 10.0])
        .show(ui, add_rows);
}

/// A chip that opens `menu` above itself when clicked. `hint` shows while
/// the menu is closed.
fn chip_menu(
    ui: &mut egui::Ui,
    id_salt: &str,
    label: &str,
    accented: bool,
    hint: &str,
    menu: impl FnOnce(&mut egui::Ui),
) {
    let popup_id = ui.scope_id().with(id_salt);
    let open = egui::Popup::is_id_open(ui.ctx(), popup_id);
    let mut response = theme::chip(ui, label, open, accented);
    if !open {
        response = response.on_hover_text(hint);
    }
    egui::Popup::from_toggle_button_response(&response)
        .id(popup_id)
        .align(egui::RectAlign::TOP_START)
        .gap(6.0)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_min_width(220.0);
            menu(ui);
        });
}

/// What the size chip says.
pub(crate) fn size_summary(max_height: Option<u32>) -> String {
    max_height.map_or_else(|| "Source size".to_owned(), |h| format!("Up to {h}p"))
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
    /// Folders saved into before, newest first, offered in the save menu.
    pub(crate) recent_dirs: Vec<PathBuf>,
    /// Tips of the empty window the user hid.
    pub(crate) hidden_tips: Vec<String>,
    pub(crate) theme: ThemeChoice,
    /// Look for a newer version once a week when hdrshot starts.
    pub(crate) update_check: bool,
    /// When a check for updates last succeeded, in seconds since 1970.
    pub(crate) last_update_check: u64,
    /// Where the window was when hdrshot last closed.
    pub(crate) window: Option<window::Place>,
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

    /// What the output chip says, e.g. "HDR PNG + SDR JPEG".
    pub(crate) fn output_summary(&self) -> String {
        let hdr = match self.hdr_format {
            Format::Png if self.discord_png => "HDR PNG for Discord",
            Format::Png => "HDR PNG",
            Format::Avif => "HDR AVIF",
            Format::UltraHdr => "Ultra HDR JPEG",
        };
        let sdr = match self.sdr_format {
            SdrKind::Png => "SDR PNG",
            SdrKind::Jpeg => "SDR JPEG",
        };
        match self.mode {
            Mode::Both => format!("{hdr} + {sdr}"),
            Mode::Hdr => hdr.to_owned(),
            Mode::Sdr => sdr.to_owned(),
        }
    }

    /// What the save chip says: the folder's name, not its whole path.
    pub(crate) fn save_summary(&self) -> String {
        if !self.save_to_folder {
            return "Next to originals".to_owned();
        }
        match &self.output_dir {
            Some(dir) => format!("In {}", folder_name(dir)),
            None => "Choose a folder".to_owned(),
        }
    }

    /// Saves into `dir` from now on and puts it first in the save menu.
    pub(crate) fn use_output_dir(&mut self, dir: PathBuf) {
        self.recent_dirs.retain(|d| d != &dir);
        self.recent_dirs.insert(0, dir.clone());
        self.recent_dirs.truncate(RECENT_DIRS);
        self.output_dir = Some(dir);
        self.save_to_folder = true;
    }

    /// The folders the save menu offers: the chosen one and the recent
    /// ones that still exist.
    pub(crate) fn save_folders(&self) -> Vec<PathBuf> {
        let mut folders: Vec<PathBuf> = Vec::new();
        for dir in self.output_dir.iter().chain(&self.recent_dirs) {
            if !folders.contains(dir) && dir.is_dir() {
                folders.push(dir.clone());
            }
        }
        folders.truncate(RECENT_DIRS);
        folders
    }

    /// What the chip for the steps after converting says.
    pub(crate) fn after_summary(&self) -> String {
        match (self.copy_to_clipboard, self.trash_originals) {
            (true, false) => "Copy to clipboard".to_owned(),
            (true, true) => format!("Clipboard, {RECYCLE_BIN}"),
            (false, true) => format!("Originals to {RECYCLE_BIN}"),
            (false, false) => "No clipboard copy".to_owned(),
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
            recent_dirs: Vec::new(),
            hidden_tips: Vec::new(),
            theme: ThemeChoice::System,
            update_check: true,
            last_update_check: 0,
            window: None,
        }
    }
}

/// Light or dark, or as the system is set.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum ThemeChoice {
    System,
    Light,
    Dark,
}

impl ThemeChoice {
    /// Uses this theme for the window's contents and its title bar.
    pub(crate) fn apply(self, ctx: &egui::Context) {
        let (preference, title_bar) = match self {
            Self::System => (
                egui::ThemePreference::System,
                egui::SystemTheme::SystemDefault,
            ),
            Self::Light => (egui::ThemePreference::Light, egui::SystemTheme::Light),
            Self::Dark => (egui::ThemePreference::Dark, egui::SystemTheme::Dark),
        };
        ctx.set_theme(preference);
        ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(title_bar));
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
            info: String::new(),
            saved: String::new(),
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

    #[test]
    fn recent_folders_come_newest_first() {
        let mut s = Settings::default();
        let base = std::env::temp_dir();
        for name in ["a", "b", "c", "d", "b"] {
            s.use_output_dir(base.join(name));
        }
        assert_eq!(
            s.recent_dirs,
            [base.join("b"), base.join("d"), base.join("c")]
        );
        assert_eq!(s.output_dir, Some(base.join("b")));
        // Only folders that exist are offered.
        s.recent_dirs.push(base.clone());
        s.output_dir = Some(base.clone());
        assert_eq!(s.save_folders(), [base]);
    }

    #[test]
    fn name_preview_lists_the_files() {
        let input = Path::new("shots").join("Screenshot 1.jxr");
        let mut s = Settings::default();
        let names = |s: &Settings| {
            let options = ConvertOptions {
                mode: s.mode.into(),
                output_dir: s.save_to_folder.then(|| s.output_dir.clone()).flatten(),
                naming: s.naming(),
                hdr: HdrOptions {
                    format: s.hdr_format.into(),
                    ..Default::default()
                },
                sdr_format: s.sdr_format.into(),
                ..Default::default()
            };
            name_preview(&input, &options)
        };
        assert_eq!(names(&s), ["Screenshot 1_hdr.png", "Screenshot 1_sdr.png"]);
        s.subfolders = true;
        s.type_suffix = false;
        s.hdr_format = Format::Avif;
        s.sdr_format = SdrKind::Jpeg;
        assert_eq!(names(&s), ["hdr/Screenshot 1.avif", "sdr/Screenshot 1.jpg"]);
        s.mode = Mode::Hdr;
        s.save_to_folder = true;
        s.output_dir = Some(std::env::temp_dir().join("out"));
        assert_eq!(names(&s), ["hdr/Screenshot 1.avif"]);
    }

    #[test]
    fn chips_sum_up_the_settings() {
        let mut s = Settings::default();
        assert_eq!(s.output_summary(), "HDR PNG + SDR PNG");
        assert_eq!(s.save_summary(), "Next to originals");
        assert_eq!(s.after_summary(), "Copy to clipboard");
        assert_eq!(size_summary(None), "Source size");
        assert_eq!(size_summary(Some(1440)), "Up to 1440p");

        s.mode = Mode::Hdr;
        s.hdr_format = Format::Avif;
        assert_eq!(s.output_summary(), "HDR AVIF");
        s.mode = Mode::Sdr;
        s.sdr_format = SdrKind::Jpeg;
        assert_eq!(s.output_summary(), "SDR JPEG");

        s.save_to_folder = true;
        s.output_dir = None;
        assert_eq!(s.save_summary(), "Choose a folder");
        s.output_dir = Some(PathBuf::from("shots").join("HDR"));
        assert_eq!(s.save_summary(), "In HDR");

        s.copy_to_clipboard = false;
        assert_eq!(s.after_summary(), "No clipboard copy");
        s.trash_originals = true;
        assert_eq!(s.after_summary(), format!("Originals to {RECYCLE_BIN}"));
    }
}
