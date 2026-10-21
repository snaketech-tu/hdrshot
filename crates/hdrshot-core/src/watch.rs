//! Watching a folder for new images, such as the Xbox Game Bar captures
//! folder, to convert each one as it is saved.
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

/// Starts watching `dir` (not its subfolders) and calls `found` from a
/// background thread with each new image once it is completely written.
pub fn watch_folder(
    dir: &Path,
    mut found: impl FnMut(PathBuf) + Send + 'static,
) -> notify::Result<FolderWatch> {
    let (tx, rx) = channel();
    let mut watcher = notify::recommended_watcher(tx)?;
    watcher.watch(dir, RecursiveMode::NonRecursive)?;
    thread::spawn(move || {
        let mut pending: HashMap<PathBuf, Pending> = HashMap::new();
        let mut reported: HashSet<PathBuf> = HashSet::new();
        loop {
            match rx.recv_timeout(POLL) {
                Ok(Ok(event)) => {
                    let new = matches!(
                        event.kind,
                        EventKind::Create(_) | EventKind::Modify(ModifyKind::Name(_))
                    );
                    for path in event.paths {
                        if !is_watched(&path) || reported.contains(&path) {
                            continue;
                        }
                        if let Some(p) = pending.get_mut(&path) {
                            p.changed = Instant::now();
                        } else if new {
                            pending.insert(path, Pending::new());
                        }
                    }
                }
                Ok(Err(_)) | Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            let now = Instant::now();
            pending.retain(|path, p| {
                let Ok(size) = std::fs::metadata(path).map(|m| m.len()) else {
                    // Gone again, e.g. a temporary file that was renamed.
                    return false;
                };
                if !p.settled(size, now) || File::open(path).is_err() {
                    return true;
                }
                reported.insert(path.clone());
                found(path.clone());
                false
            });
        }
    });
    Ok(FolderWatch { _watcher: watcher })
}

/// Whether a new file at `path` is one to report.
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
        assert!(!is_watched(Path::new("Captures/game_sdr.jpg")));
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
}
