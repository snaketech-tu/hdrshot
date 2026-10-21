//! Background conversion, so the window stays responsive while files are
//! decoded and encoded.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread;

use hdrshot_core::{ConvertOptions, Outputs, share};

pub enum Event {
    Started(usize),
    Finished(usize, Result<Outputs, String>),
    /// The batch is over; carries the outcome of copying the last image
    /// to the clipboard, if that was asked for.
    Done(Option<Copied>),
}

/// Kept by the app for as long as it runs: on Linux a copied image is only
/// available while its `Clipboard` exists.
pub type SharedClipboard = Arc<Mutex<Option<arboard::Clipboard>>>;

/// A running batch. Dropping it does not stop the thread; call `cancel`.
pub struct Job {
    pub events: Receiver<Event>,
    cancel: Arc<AtomicBool>,
}

impl Job {
    /// Convert `files` one after another (the core already uses every CPU
    /// core per image). Each event carries the file's index in `files`.
    /// With `clipboard`, the last PNG written is copied to it.
    pub fn start(
        files: Vec<(usize, PathBuf)>,
        options: ConvertOptions,
        clipboard: Option<SharedClipboard>,
        ctx: egui::Context,
    ) -> Self {
        let (tx, events) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        thread::spawn(move || {
            let send = |tx: &Sender<Event>, event| {
                let _ = tx.send(event);
                ctx.request_repaint();
            };
            // The last file converted and its PNGs.
            let mut last = None;
            for (index, path) in files {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                send(&tx, Event::Started(index));
                let result = hdrshot_core::convert_file_with_discord_png(&path, &options).map(
                    |(outputs, discord)| {
                        if let (Some(hdr), Some(png)) = (&outputs.hdr, discord) {
                            save_discord_png(hdr, &png);
                        }
                        outputs
                    },
                );
                if let Ok(outputs) = &result
                    && (outputs.hdr.is_some() || outputs.sdr.is_some())
                {
                    last = Some((path.clone(), outputs.clone()));
                }
                send(
                    &tx,
                    Event::Finished(index, result.map_err(|e| e.to_string())),
                );
            }
            let copied = clipboard
                .zip(last)
                .map(|(clipboard, (input, outputs))| copy(&clipboard, &input, &outputs, &options));
            send(&tx, Event::Done(copied));
        });
        Self { events, cancel }
    }

    /// Stop after the file that is currently being converted.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

/// The outcome of a clipboard copy.
pub struct Copied {
    pub message: String,
    /// Set when the copy is too large for Discord, to offer a smaller one.
    pub too_big: Option<TooBig>,
}

impl From<String> for Copied {
    fn from(message: String) -> Self {
        Self {
            message,
            too_big: None,
        }
    }
}

/// Folder for the Discord PNGs of AVIF and Ultra HDR outputs, which are
/// made while converting so that copying them is instant. Emptied when the
/// app starts.
fn discord_dir() -> PathBuf {
    std::env::temp_dir().join("hdrshot-discord")
}

/// Where the Discord PNG of the HDR output `hdr` is kept: named like it,
/// in a subfolder per output path so outputs of the same name don't mix.
fn discord_path(hdr: &Path) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    hdr.hash(&mut hasher);
    discord_dir()
        .join(format!("{:016x}", hasher.finish()))
        .join(hdr.with_extension("png").file_name().unwrap_or_default())
}

fn save_discord_png(hdr: &Path, png: &[u8]) {
    let path = discord_path(hdr);
    if let Some(dir) = path.parent()
        && std::fs::create_dir_all(dir).is_ok()
    {
        let _ = std::fs::write(&path, png);
    }
}

/// Deletes the Discord PNGs of an earlier run.
pub fn clean_up() {
    let _ = std::fs::remove_dir_all(discord_dir());
}

/// A copied PNG over Discord's upload limit.
#[derive(Clone)]
pub struct TooBig {
    /// The JXR it was made from.
    pub input: PathBuf,
    /// The copied PNG's file name.
    pub name: String,
    pub hdr: bool,
    pub size: u64,
}

/// Put one of `outputs`, made from `input`, on the clipboard as a file,
/// preferring the HDR one, so pasting it (into Discord, a browser or a
/// folder) gives the full image, HDR included. An HDR PNG is copied without
/// its `cICP` chunk so that it also shows right on Discord. AVIF and Ultra
/// HDR outputs are copied as such a PNG too, because Discord shows AVIF too
/// bright and Ultra HDR JPEGs in SDR only: the one made while converting,
/// or one made from `input` now if there is none.
pub fn copy(
    clipboard: &SharedClipboard,
    input: &Path,
    outputs: &Outputs,
    options: &ConvertOptions,
) -> Copied {
    // The HDR PNG when there is one: it is the faithful copy.
    let (path, hdr) = match (&outputs.hdr, &outputs.sdr) {
        (Some(hdr), _) => (hdr.as_path(), true),
        (None, Some(sdr)) => (sdr.as_path(), false),
        (None, None) => return Copied::from("Nothing to copy".to_string()),
    };
    let png = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("png"));
    let name = if png {
        path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    } else {
        path.with_extension("png")
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    };
    let ready = discord_path(path);
    let copy = if !hdr {
        Ok(path.to_path_buf())
    } else if png {
        std::fs::read(path).and_then(|bytes| share::clipboard_file(&name, &bytes, true))
    } else if ready.is_file() {
        Ok(ready)
    } else {
        share::discord_png(input, options)
            .map_err(std::io::Error::other)
            .and_then(|bytes| share::clipboard_file(&name, &bytes, false))
    };
    let copy = match copy {
        Ok(copy) => copy,
        Err(e) => return Copied::from(format!("Could not copy to the clipboard: {e}")),
    };
    if let Err(e) = set_file(clipboard, copy.clone()) {
        return Copied::from(e);
    }
    let size = std::fs::metadata(&copy).map_or(0, |m| m.len());
    if size > share::DISCORD_LIMIT {
        Copied {
            message: format!(
                "Copied {name} ({}), more than Discord's {} limit",
                share::megabytes(size),
                share::megabytes(share::DISCORD_LIMIT)
            ),
            too_big: Some(TooBig {
                input: input.to_path_buf(),
                name,
                hdr,
                size,
            }),
        }
    } else {
        Copied::from(format!("Copied {name} to the clipboard"))
    }
}

/// Copy a smaller version of `too_big` that fits Discord's limit.
pub fn copy_smaller(
    clipboard: &SharedClipboard,
    too_big: &TooBig,
    options: &ConvertOptions,
) -> String {
    let fail = |e: &dyn std::fmt::Display| format!("Could not make a smaller copy: {e}");
    let smaller =
        match share::smaller_png(&too_big.input, options, too_big.hdr, share::DISCORD_LIMIT) {
            Ok(Some(smaller)) => smaller,
            Ok(None) => return "Even a 720p copy is larger than 10 MB".into(),
            Err(e) => return fail(&e),
        };
    let path = match share::clipboard_file(&too_big.name, &smaller.png, false) {
        Ok(path) => path,
        Err(e) => return fail(&e),
    };
    match set_file(clipboard, path) {
        Ok(()) => format!(
            "Copied {} at {}×{} ({})",
            too_big.name,
            smaller.width,
            smaller.height,
            share::megabytes(smaller.png.len() as u64)
        ),
        Err(e) => e,
    }
}

fn set_file(clipboard: &SharedClipboard, path: PathBuf) -> Result<(), String> {
    let fail = |e: arboard::Error| format!("Could not copy to the clipboard: {e}");
    let mut clipboard = clipboard.lock().unwrap_or_else(|e| e.into_inner());
    let clipboard = match &mut *clipboard {
        Some(c) => c,
        None => clipboard.insert(arboard::Clipboard::new().map_err(fail)?),
    };
    clipboard.set().file_list(&[path]).map_err(fail)
}

/// Run `copy` on a background thread (reading, rewriting and maybe
/// shrinking the PNG takes a moment) and send its outcome to `messages`.
pub fn in_background(
    messages: Sender<Copied>,
    ctx: egui::Context,
    copy: impl FnOnce() -> Copied + Send + 'static,
) {
    thread::spawn(move || {
        let _ = messages.send(copy());
        ctx.request_repaint();
    });
}
