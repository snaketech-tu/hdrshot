//! The watch folder: new images saved there, such as Game Bar screenshots,
//! are added to the list and converted with the current settings. Files
//! from Explorer's "Convert with hdrshot" take the same way.

use hdrshot_core::watch::{FolderWatch, watch_folder};

use super::*;

/// How long to wait for more files after one arrives before converting.
const GATHER_SECONDS: f64 = 0.4;

/// The folder being watched and its watch.
pub(crate) struct Watching {
    pub(crate) dir: PathBuf,
    _watch: FolderWatch,
}

impl App {
    /// Starts or stops watching to match the settings.
    pub(crate) fn sync_watch(&mut self, ctx: &egui::Context) {
        let wanted = self
            .settings
            .watch
            .then(|| self.settings.watch_dir.clone())
            .flatten();
        if self.watching.as_ref().map(|w| &w.dir) == wanted.as_ref() {
            return;
        }
        self.watching = None;
        let Some(dir) = wanted else { return };
        let sender = self.watched.0.clone();
        let repaint = ctx.clone();
        match watch_folder(&dir, move |path| {
            let _ = sender.send(path);
            repaint.request_repaint();
        }) {
            Ok(watch) => self.watching = Some(Watching { dir, _watch: watch }),
            Err(e) => {
                self.settings.watch = false;
                self.show_toast(format!("Can't watch {}: {e}", dir.display()), ctx);
            }
        }
    }

    /// Adds new images from the watch folder or from "Convert with hdrshot"
    /// and converts them once no batch is running, as a batch of their
    /// own. Files arriving close together, as Explorer starts one launch
    /// per selected file, become one batch.
    pub(crate) fn receive_watched(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        let before = self.waiting.len();
        self.waiting.extend(self.watched.1.try_iter());
        if self.waiting.len() > before {
            self.last_arrival = now;
        }
        if self.waiting.is_empty() || self.running() || self.asking.is_some() {
            return;
        }
        let quiet = now - self.last_arrival;
        if quiet < GATHER_SECONDS {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(GATHER_SECONDS - quiet));
            return;
        }
        let paths = std::mem::take(&mut self.waiting);
        for path in &paths {
            self.add_file(path.clone());
        }
        if !self.can_convert() {
            // Saving to a folder that isn't chosen yet; they wait in the list.
            return;
        }
        let indices = paths
            .iter()
            .filter_map(|p| self.files.iter().position(|e| &e.path == p))
            .collect();
        self.start_some(indices, ctx);
    }

    /// The "Watch folder" row of the settings.
    pub(crate) fn watch_ui(&mut self, ui: &mut egui::Ui) {
        let s = &mut self.settings;
        ui.horizontal(|ui| {
            ui.add_enabled(
                s.watch_dir.is_some(),
                egui::Checkbox::new(&mut s.watch, "Convert new images in"),
            )
            .on_hover_text(
                "Convert each image saved to this folder from now on, such as a new \
                 Game Bar screenshot, with these settings. Images already in it are left \
                 alone. Works while hdrshot is open.",
            )
            .on_disabled_hover_text("Choose a folder to watch first");
            let label = match &s.watch_dir {
                Some(dir) => dir.display().to_string(),
                None => "Choose a folder…".to_owned(),
            };
            if ui
                .add(egui::Button::new(label).truncate())
                .on_hover_text("Choose the folder to watch")
                .clicked()
            {
                let mut dialog = rfd::FileDialog::new().set_title("Watch folder");
                if let Some(dir) = s.watch_dir.clone().or_else(default_watch_dir) {
                    dialog = dialog.set_directory(dir);
                }
                if let Some(dir) = dialog.pick_folder() {
                    s.watch_dir = Some(dir);
                    s.watch = true;
                }
            }
        });
    }
}

/// The Xbox Game Bar captures folder, if it exists: where Windows saves
/// HDR screenshots.
pub(crate) fn default_watch_dir() -> Option<PathBuf> {
    if !cfg!(windows) {
        return None;
    }
    let dir = PathBuf::from(std::env::var_os("USERPROFILE")?)
        .join("Videos")
        .join("Captures");
    dir.is_dir().then_some(dir)
}
