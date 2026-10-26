//! Watching a folder and its subfolders for new images, such as the Xbox
//! Game Bar captures folder or the NVIDIA app's folder with one subfolder
//! per game, to convert each one as it is saved.
//!
//! A new file is reported once, when it looks finished: its size hasn't
//! changed for [`SETTLE`] and it can be opened. Only files that appear after
//! watching starts are reported, never the folder's existing ones. PNGs are
//! left out, as with [`find_inputs`](crate::find_inputs): they are both an
//! input type and what hdrshot itself writes into the folder.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::thread;
use std::time::{Duration, Instant};

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher, event::ModifyKind};

use crate::input::InputFormat;

/// How long a new file's size must stay the same before it is reported.
pub const SETTLE: Duration = Duration::from_secs(1);

/// How often files still being written are checked.
const POLL: Duration = Duration::from_millis(250);

/// Watches one folder until dropped.
pub struct FolderWatch {
    // Dropping the watcher closes the event channel, which ends the thread.
    _watcher: RecommendedWatcher,
}

/// Starts watching `dir` and its subfolders and calls `found` from a
/// background thread with each new image once it is completely written.
pub fn watch_folder(
    dir: &Path,
    mut found: impl FnMut(PathBuf) + Send + 'static,
) -> notify::Result<FolderWatch> {
    let (tx, rx) = channel();
    let mut watcher = notify::recommended_watcher(tx)?;
    watcher.watch(dir, RecursiveMode::Recursive)?;
    let mut tracker = Tracker::new(dir);
    thread::spawn(move || {
        loop {
            match rx.recv_timeout(POLL) {
                Ok(Ok(event)) => tracker.note(event),
                Ok(Err(_)) | Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            tracker.report_settled(Instant::now(), &mut found);
        }
    });
    Ok(FolderWatch { _watcher: watcher })
}

/// What the watch knows of the folder's files.
struct Tracker {
    /// The files from before watching started. macOS can report a change
    /// to one of them as a creation, so they are left out until deleted or
    /// renamed away.
    existing: Existing,
    /// New files that may still be being written.
    pending: HashMap<PathBuf, Pending>,
    reported: HashSet<PathBuf>,
}

impl Tracker {
    fn new(dir: &Path) -> Self {
        Self {
            existing: Existing::new(dir),
            pending: HashMap::new(),
            reported: HashSet::new(),
        }
    }

    /// Takes note of a file event.
    fn note(&mut self, event: notify::Event) {
        let created = matches!(event.kind, EventKind::Create(_));
        let new = created || matches!(event.kind, EventKind::Modify(ModifyKind::Name(_)));
        for path in event.paths {
            // A new subfolder, such as the folder of a game's first
            // screenshot: files saved in it before it was watched too are
            // found by looking inside. A folder renamed or moved in keeps
            // its files unreported. macOS can also report the watched
            // folder itself as created, so its old files are skipped.
            if created && path.is_dir() {
                for file in files_under(&path) {
                    self.add_new(file);
                }
                continue;
            }
            if let Some(p) = self.pending.get_mut(&path) {
                p.changed = Instant::now();
            } else if new {
                self.add_new(path);
            }
        }
    }

    /// Starts following `path` as a new file, if it is one to report.
    fn add_new(&mut self, path: PathBuf) {
        if self.existing.forget_if_gone(&path)
            || !is_watched(&path)
            || self.reported.contains(&path)
        {
            return;
        }
        self.pending.entry(path).or_insert_with(Pending::new);
    }

    /// Reports the new files that are completely written, through `found`.
    fn report_settled(&mut self, now: Instant, found: &mut impl FnMut(PathBuf)) {
        self.pending.retain(|path, p| {
            let Ok(size) = std::fs::metadata(path).map(|m| m.len()) else {
                // Gone again, e.g. a temporary file that was renamed.
                return false;
            };
            if !p.settled(size, now) || File::open(path).is_err() {
                return true;
            }
            self.reported.insert(path.clone());
            // A JPEG or AVIF, now complete, may turn out to be one hdrshot
            // wrote or one without a gain map.
            if !crate::input::is_scan_input(path) {
                return false;
            }
            found(path.clone());
            false
        });
    }
}

/// The files of the watched folder from before watching started, by path
/// from the folder.
struct Existing {
    /// Spellings of the watched folder that event paths may start with,
    /// e.g. both `/var/...` and `/private/var/...` on macOS.
    roots: Vec<PathBuf>,
    files: HashSet<PathBuf>,
}

impl Existing {
    fn new(dir: &Path) -> Self {
        let files = files_under(dir)
            .iter()
            .filter_map(|f| Some(f.strip_prefix(dir).ok()?.to_path_buf()))
            .collect();
        let mut roots = vec![dir.to_path_buf()];
        roots.extend(std::path::absolute(dir).ok());
        roots.extend(dir.canonicalize().ok());
        Self { roots, files }
    }

    /// Whether `path` is one of the files, which it stops being once it is
    /// gone.
    fn forget_if_gone(&mut self, path: &Path) -> bool {
        let relative = match self.roots.iter().find_map(|r| path.strip_prefix(r).ok()) {
            Some(relative) => relative.to_path_buf(),
            // A spelling not foreseen: matched by its end.
            None => match self.files.iter().find(|f| path.ends_with(f)) {
                Some(file) => file.clone(),
                None => return false,
            },
        };
        if !self.files.contains(&relative) {
            return false;
        }
        if !path.exists() {
            self.files.remove(&relative);
        }
        true
    }
}

/// The files in `dir` and its subfolders, without following links to
/// folders.
fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut folders = vec![dir.to_path_buf()];
    while let Some(folder) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => folders.push(entry.path()),
                Ok(_) => files.push(entry.path()),
                Err(_) => {}
            }
        }
    }
    files
}

/// Whether a new file at `path` may be one to report: JPEGs and AVIFs are
/// checked once written ([`crate::input::is_scan_input`]).
fn is_watched(path: &Path) -> bool {
    let hidden = path
        .file_name()
        .is_some_and(|n| n.to_string_lossy().starts_with('.'));
    !hidden && InputFormat::from_path(path).is_some_and(|f| f != InputFormat::Png)
}

/// A new file that may still be being written.
struct Pending {
    /// Its size at the last check, if it has been checked.
    size: Option<u64>,
    /// When its size or a file event last showed it changing.
    changed: Instant,
}

impl Pending {
    fn new() -> Self {
        Self {
            size: None,
            changed: Instant::now(),
        }
    }

    /// Records the file's current `size` and returns whether it has stayed
    /// the same, and not empty, for [`SETTLE`].
    fn settled(&mut self, size: u64, now: Instant) -> bool {
        if self.size != Some(size) {
            self.size = Some(size);
            self.changed = now;
            return false;
        }
        size > 0 && now.duration_since(self.changed) >= SETTLE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    #[test]
    fn watched_files() {
        assert!(is_watched(Path::new("Captures/game.jxr")));
        assert!(is_watched(Path::new("Captures/render.EXR")));
        assert!(!is_watched(Path::new("Captures/game_hdr.png")));
        assert!(is_watched(Path::new("Captures/photo.jpg")));
        assert!(!is_watched(Path::new("Captures/.game_hdr.png.12-0.tmp")));
        assert!(!is_watched(Path::new("Captures/clip.mp4")));
    }

    #[test]
    fn settles_only_after_the_size_stops_changing() {
        let start = Instant::now();
        let mut p = Pending::new();
        assert!(!p.settled(100, start));
        assert!(!p.settled(100, start + SETTLE / 2));
        assert!(!p.settled(200, start + SETTLE));
        assert!(!p.settled(200, start + SETTLE + SETTLE / 2));
        assert!(p.settled(200, start + SETTLE * 2));
        let mut empty = Pending::new();
        assert!(!empty.settled(0, start));
        assert!(!empty.settled(0, start + SETTLE * 2));
    }

    #[test]
    fn reports_new_images_once_written() {
        let dir = std::env::temp_dir().join(format!("hdrshot-watch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // Event paths are canonical on macOS (/private/var/...).
        let dir = dir.canonicalize().unwrap();
        std::fs::write(dir.join("old.jxr"), b"existing").unwrap();
        let found = Arc::new(Mutex::new(Vec::new()));
        let sink = found.clone();
        let watch = watch_folder(&dir, move |path| sink.lock().unwrap().push(path)).unwrap();

        // Written in two parts with a pause, like a slow save.
        let mut file = File::create(dir.join("new.jxr")).unwrap();
        file.write_all(b"first part").unwrap();
        file.flush().unwrap();
        thread::sleep(SETTLE / 2);
        assert!(found.lock().unwrap().is_empty(), "reported while writing");
        file.write_all(b", second part").unwrap();
        drop(file);
        std::fs::write(dir.join("new_hdr.png"), b"output").unwrap();
        std::fs::write(dir.join("new_sdr.jpg"), b"\xff\xd8\xff\xe0 plain").unwrap();
        std::fs::write(dir.join("old.jxr"), b"changed").unwrap();

        let deadline = Instant::now() + SETTLE * 5;
        while found.lock().unwrap().is_empty() && Instant::now() < deadline {
            thread::sleep(POLL);
        }
        thread::sleep(SETTLE);
        assert_eq!(*found.lock().unwrap(), [dir.join("new.jxr")]);
        drop(watch);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reports_new_images_in_subfolders() {
        let dir = std::env::temp_dir().join(format!("hdrshot-watch-sub-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Old Game")).unwrap();
        let dir = dir.canonicalize().unwrap();
        std::fs::write(dir.join("Old Game").join("old.jxr"), b"existing").unwrap();
        let found = Arc::new(Mutex::new(Vec::new()));
        let sink = found.clone();
        let watch = watch_folder(&dir, move |path| sink.lock().unwrap().push(path)).unwrap();

        // As the NVIDIA app does for a game's first screenshot: a new
        // folder, and the screenshot in it right away.
        let new_game = dir.join("New Game");
        std::fs::create_dir(&new_game).unwrap();
        std::fs::write(new_game.join("first.jxr"), b"screenshot").unwrap();
        std::fs::write(dir.join("Old Game").join("second.jxr"), b"screenshot").unwrap();
        std::fs::write(dir.join("Old Game").join("old.jxr"), b"changed").unwrap();

        let deadline = Instant::now() + SETTLE * 5;
        while found.lock().unwrap().len() < 2 && Instant::now() < deadline {
            thread::sleep(POLL);
        }
        thread::sleep(SETTLE);
        let mut found = found.lock().unwrap().clone();
        found.sort();
        assert_eq!(
            found,
            [
                new_game.join("first.jxr"),
                dir.join("Old Game").join("second.jxr")
            ]
        );
        drop(watch);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn old_files_stay_unreported_when_their_folder_shows_up_as_new() {
        use notify::event::{CreateKind, Event};
        let dir = std::env::temp_dir().join(format!("hdrshot-tracker-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Old Game")).unwrap();
        std::fs::write(dir.join("old.jxr"), b"existing").unwrap();
        std::fs::write(dir.join("Old Game").join("old.jxr"), b"existing").unwrap();
        let mut tracker = Tracker::new(&dir);
        let created =
            |path: PathBuf| Event::new(EventKind::Create(CreateKind::Folder)).add_path(path);

        // As macOS can report for the watched folder, from just before
        // watching started.
        tracker.note(created(dir.clone()));
        tracker.note(created(dir.join("Old Game")));
        assert!(tracker.pending.is_empty());

        let new_game = dir.join("New Game");
        std::fs::create_dir(&new_game).unwrap();
        std::fs::write(new_game.join("first.jxr"), b"screenshot").unwrap();
        std::fs::write(new_game.join("first_hdr.png"), b"output").unwrap();
        tracker.note(created(new_game.clone()));
        let pending: Vec<_> = tracker.pending.keys().cloned().collect();
        assert_eq!(pending, [new_game.join("first.jxr")]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn existing_files_by_any_spelling() {
        let dir = std::env::temp_dir().join(format!("hdrshot-existing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Game")).unwrap();
        std::fs::write(dir.join("Game").join("old.jxr"), b"existing").unwrap();
        std::fs::write(dir.join("top.jxr"), b"existing").unwrap();
        let mut existing = Existing::new(&dir);
        assert!(existing.forget_if_gone(&dir.join("Game").join("old.jxr")));
        assert!(existing.forget_if_gone(&dir.canonicalize().unwrap().join("top.jxr")));
        // A folder spelled some other way, matched by the path's end.
        assert!(existing.forget_if_gone(Path::new("/elsewhere/Game/old.jxr")));
        assert!(!existing.forget_if_gone(&dir.join("Game").join("top.jxr")));
        assert!(!existing.forget_if_gone(&dir.join("new.jxr")));
        std::fs::remove_file(dir.join("top.jxr")).unwrap();
        assert!(existing.forget_if_gone(&dir.join("top.jxr")));
        assert!(!existing.forget_if_gone(&dir.join("top.jxr")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
