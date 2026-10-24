//! The watch folder: new images saved there or in its subfolders, such as
//! Game Bar and NVIDIA app screenshots, are added to the list and converted
//! with the current settings. Files from Explorer's "Convert with hdrshot"
//! take the same way.

use std::sync::OnceLock;
use std::time::SystemTime;

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
    /// Starts or stops watching to match the settings and the pause.
    pub(crate) fn sync_watch(&mut self, ctx: &egui::Context) {
        if !self.settings.watch {
            self.watch_paused = false;
        }
        let wanted = (self.settings.watch && !self.watch_paused)
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

    /// The pill in the header while the watch folder is on: a green dot
    /// and the folder's name. Clicking it pauses watching until clicked
    /// again or hdrshot restarts.
    pub(crate) fn watch_pill(&mut self, ui: &mut egui::Ui) {
        let Some(dir) = self
            .settings
            .watch_dir
            .as_ref()
            .filter(|_| self.settings.watch)
        else {
            return;
        };
        let paused = self.watch_paused;
        let mut name = folder_name(dir);
        if name.chars().count() > 18 {
            name = name.chars().take(17).collect::<String>() + "…";
        }
        let text = if paused {
            "Watching paused".to_owned()
        } else {
            format!("Watching {name}")
        };
        let font = egui::FontId::proportional(12.5);
        let galley = ui
            .painter()
            .layout_no_wrap(text, font, Color32::PLACEHOLDER);
        let size = Vec2::new(galley.size().x + 34.0, theme::ROW_HEIGHT);
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        let visuals = ui.visuals();
        let ok = ok_color(ui);
        let (fill, dot) = if paused {
            (visuals.widgets.inactive.bg_fill, visuals.weak_text_color())
        } else {
            (ok.gamma_multiply(0.14), ok)
        };
        let fill = if response.hovered() {
            visuals.widgets.hovered.bg_fill
        } else {
            fill
        };
        let painter = ui.painter();
        painter.rect_filled(rect, size.y / 2.0, fill);
        painter.circle_filled(egui::pos2(rect.left() + 14.0, rect.center().y), 3.5, dot);
        let text_pos = egui::pos2(rect.left() + 24.0, rect.center().y - galley.size().y / 2.0);
        painter.galley_with_override_text_color(text_pos, galley, visuals.text_color());
        theme::focus_ring(ui, &response, size.y / 2.0);
        let hint = if paused {
            format!(
                "Not watching {} now. Click to watch it again.",
                dir.display()
            )
        } else {
            format!(
                "Converting new images saved in {}. Click to pause.",
                dir.display()
            )
        };
        if response
            .on_hover_text(hint)
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            self.watch_paused = !paused;
        }
    }

    /// The watch folder setting, on the Settings page: a menu of the
    /// screenshot folders found and any other folder.
    pub(crate) fn watch_ui(&mut self, ui: &mut egui::Ui) {
        let s = &mut self.settings;
        ui.horizontal(|ui| {
            ui.add_enabled(
                s.watch_dir.is_some(),
                egui::Checkbox::new(&mut s.watch, "Convert new images saved in"),
            )
            .on_hover_text(format!(
                "Convert each image saved to this folder or its subfolders from now \
                 on, such as a new Game Bar or NVIDIA app screenshot, with these \
                 settings. Images already in it are left alone. Works while hdrshot \
                 is open{}.",
                if tray::SUPPORTED {
                    ", or running in the tray (see Background)"
                } else {
                    ""
                }
            ))
            .on_disabled_hover_text("Choose a folder to watch first");
            let label = match &s.watch_dir {
                Some(dir) => dir.display().to_string(),
                None => "Choose a folder…".to_owned(),
            };
            egui::ComboBox::from_id_salt("watch folder")
                .selected_text(label)
                .width(ui.available_width().min(300.0))
                .truncate()
                .show_ui(ui, |ui| {
                    for folder in capture_folders() {
                        let selected = s.watch_dir.as_ref() == Some(&folder.dir);
                        if ui
                            .selectable_label(selected, format!("{} screenshots", folder.app))
                            .on_hover_text(folder.dir.display().to_string())
                            .clicked()
                        {
                            s.watch_dir = Some(folder.dir.clone());
                            s.watch = true;
                        }
                    }
                    if !capture_folders().is_empty() {
                        ui.separator();
                    }
                    if ui.button("Choose a folder…").clicked() {
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
        });
    }
}

/// A folder where an app saves HDR screenshots.
pub(crate) struct CaptureFolder {
    /// The app's name, e.g. "Game Bar".
    pub(crate) app: &'static str,
    pub(crate) dir: PathBuf,
}

/// The screenshot folders on this computer (Windows only), the one holding
/// the newest JPEG XR screenshot first. Looked up once.
pub(crate) fn capture_folders() -> &'static [CaptureFolder] {
    static FOLDERS: OnceLock<Vec<CaptureFolder>> = OnceLock::new();
    FOLDERS.get_or_init(|| {
        let videos = std::env::var_os("USERPROFILE")
            .filter(|_| cfg!(windows))
            .map(|home| PathBuf::from(home).join("Videos"));
        videos.map_or_else(Vec::new, |videos| capture_folders_in(&videos))
    })
}

/// The screenshot folder suggested to watch.
pub(crate) fn default_watch_dir() -> Option<PathBuf> {
    capture_folders().first().map(|f| f.dir.clone())
}

/// The screenshot folders in the user's `videos` folder: Game Bar's
/// `Captures` and the NVIDIA app's `NVIDIA`, which has a subfolder per
/// game. The one with the newest `.jxr` file comes first, then Game Bar's.
fn capture_folders_in(videos: &Path) -> Vec<CaptureFolder> {
    let mut found: Vec<_> = [("Game Bar", "Captures", 0), ("NVIDIA app", "NVIDIA", 1)]
        .into_iter()
        .map(|(app, name, depth)| (app, videos.join(name), depth))
        .filter(|(_, dir, _)| dir.is_dir())
        .map(|(app, dir, depth)| (newest_jxr(&dir, depth), CaptureFolder { app, dir }))
        .collect();
    // Stable, so folders without screenshots keep their order.
    found.sort_by(|(a, _), (b, _)| b.cmp(a));
    found.into_iter().map(|(_, folder)| folder).collect()
}

/// When the newest `.jxr` file in `dir`, or in its subfolders down to
/// `depth` levels, was saved.
fn newest_jxr(dir: &Path, depth: usize) -> Option<SystemTime> {
    let mut newest = None;
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if kind.is_dir() {
            if depth > 0 {
                newest = newest.max(newest_jxr(&path, depth - 1));
            }
        } else if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("jxr"))
        {
            newest = newest.max(entry.metadata().ok().and_then(|m| m.modified().ok()));
        }
    }
    newest
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn newest_screenshots_first() {
        let videos = std::env::temp_dir().join(format!("hdrshot-videos-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&videos);
        let game = videos.join("NVIDIA").join("Some Game");
        std::fs::create_dir_all(&game).unwrap();
        std::fs::create_dir_all(videos.join("Captures")).unwrap();
        let apps = |videos: &Path| -> Vec<_> {
            capture_folders_in(videos).iter().map(|f| f.app).collect()
        };
        // No screenshots yet: Game Bar first.
        assert_eq!(apps(&videos), ["Game Bar", "NVIDIA app"]);

        let save = |path: PathBuf, age: u64| {
            let file = std::fs::File::create(path).unwrap();
            let time = SystemTime::now() - Duration::from_secs(age);
            file.set_modified(time).unwrap();
        };
        save(videos.join("Captures").join("old.jxr"), 3600);
        save(game.join("new.jxr"), 60);
        save(game.join("clip.mp4"), 0);
        assert_eq!(apps(&videos), ["NVIDIA app", "Game Bar"]);
        save(videos.join("Captures").join("newer.JXR"), 10);
        assert_eq!(apps(&videos), ["Game Bar", "NVIDIA app"]);

        std::fs::remove_dir_all(videos.join("Captures")).unwrap();
        assert_eq!(apps(&videos), ["NVIDIA app"]);
        let _ = std::fs::remove_dir_all(&videos);
    }
}
