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
    /// The original of a converted file was moved to the Recycle Bin, or
    /// why it couldn't be.
    Trashed(usize, Result<(), String>),
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
    thread: thread::JoinHandle<()>,
}

impl Job {
    /// Convert `files` one after another (the core already uses every CPU
    /// core per image). Each event carries the file's index in `files`.
    /// With `clipboard`, the last PNG written is copied to it. With
    /// `trash`, the originals whose outputs were all written are moved to
    /// the Recycle Bin at the end, unless the batch was stopped.
    pub fn start(
        files: Vec<(usize, PathBuf)>,
        options: ConvertOptions,
        clipboard: Option<SharedClipboard>,
        trash: bool,
        ctx: egui::Context,
    ) -> Self {
        let (tx, events) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        let thread = thread::spawn(move || {
            let send = |tx: &Sender<Event>, event| {
                let _ = tx.send(event);
                ctx.request_repaint();
            };
            // The last file converted and its PNGs.
            let mut last = None;
            // Files whose outputs were all written, for `trash`.
            let mut complete = Vec::new();
            let count = files.len();
            for (n, (index, path)) in files.into_iter().enumerate() {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                send(&tx, Event::Started(index));
                // The Discord PNG of an AVIF or Ultra HDR result is made from
                // the image already decoded, but only for the last file, the
                // one copied after the batch. Other rows make theirs when
                // they are copied, from the original, so with `trash` every
                // file gets one now.
                let result = if trash || (clipboard.is_some() && n + 1 == count) {
                    hdrshot_core::convert_file_with_discord_png(&path, &options).map(
                        |(outputs, discord)| {
                            if let (Some(hdr), Some(png)) = (&outputs.hdr, discord) {
                                save_discord_png(hdr, &png);
                            }
                            outputs
                        },
                    )
                } else {
                    hdrshot_core::convert_file(&path, &options)
                };
                if let Ok(outputs) = &result
                    && (outputs.hdr.is_some() || outputs.sdr.is_some())
                {
                    last = Some((path.clone(), outputs.clone()));
                }
                if result.as_ref().is_ok_and(Outputs::complete) {
                    complete.push((index, path.clone()));
                }
                send(
                    &tx,
                    Event::Finished(index, result.map_err(|e| e.to_string())),
                );
            }
            let copied = clipboard
                .zip(last)
                .map(|(clipboard, (input, outputs))| copy(&clipboard, &input, &outputs, &options));
            // After the copy, which may still read the last original.
            if trash && !stop.load(Ordering::Relaxed) {
                for (index, path) in complete {
                    let result = ::trash::delete(&path).map_err(|e| e.to_string());
                    send(&tx, Event::Trashed(index, result));
                }
            }
            send(&tx, Event::Done(copied));
        });
        Self {
            events,
            cancel,
            thread,
        }
    }

    /// Stop after the file that is currently being converted.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// Stops after the file that is currently being converted and waits
    /// until it is written.
    pub fn finish(self) {
        self.cancel();
        let _ = self.thread.join();
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

/// Folder for the Discord PNGs of AVIF and Ultra HDR outputs made while
/// converting, so that copying the last file of a batch is instant.
/// Emptied when the app starts.
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

/// The Discord PNG of the HDR output `hdr`, if made since `hdr` was last
/// written.
fn ready_discord_png(hdr: &Path) -> Option<PathBuf> {
    let modified = |path: &Path| std::fs::metadata(path).and_then(|m| m.modified()).ok();
    let ready = discord_path(hdr);
    (modified(&ready)? >= modified(hdr)?).then_some(ready)
}

/// Whether copying `outputs` first has to make the Discord PNG of an AVIF or
/// Ultra HDR result, which takes a second or two.
pub fn copy_makes_png(outputs: &Outputs) -> bool {
    outputs.hdr.as_ref().is_some_and(|hdr| {
        !hdr.extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("png"))
            && ready_discord_png(hdr).is_none()
    })
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

/// The file of `outputs`, made from `input`, that Copy for Discord and
/// dragging the row out give: the HDR one if made, so pasting or dropping
/// it (into Discord, a browser or a folder) gives the full image, HDR
/// included. An HDR PNG is given without its `cICP` chunk so that it also
/// shows right on Discord. AVIF and Ultra HDR outputs are given as such a
/// PNG too, because Discord shows AVIF too bright and Ultra HDR JPEGs in
/// SDR only: the one made while converting, or one made from `input` now
/// if there is none. Returns the file and whether it is HDR.
pub fn discord_file(
    input: &Path,
    outputs: &Outputs,
    options: &ConvertOptions,
) -> Result<(PathBuf, bool), String> {
    let path = match (&outputs.hdr, &outputs.sdr) {
        (Some(hdr), _) => hdr.as_path(),
        (None, Some(sdr)) => return Ok((sdr.clone(), false)),
        (None, None) => return Err("Nothing to copy".to_owned()),
    };
    if let Some(ready) = ready_discord_png(path) {
        return Ok((ready, true));
    }
    let png = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("png"));
    let made = if png {
        std::fs::read(path).map(|bytes| hdrshot_core::for_discord(&bytes))
    } else {
        share::discord_png(input, options).map_err(std::io::Error::other)
    };
    let ready = discord_path(path);
    made.and_then(|png| {
        std::fs::create_dir_all(ready.parent().unwrap_or(Path::new(".")))?;
        std::fs::write(&ready, png)
    })
    .map(|()| (ready, true))
    .map_err(|e| e.to_string())
}

/// Put [`discord_file`] of `outputs` on the clipboard as a file.
pub fn copy(
    clipboard: &SharedClipboard,
    input: &Path,
    outputs: &Outputs,
    options: &ConvertOptions,
) -> Copied {
    let (copy, hdr) = match discord_file(input, outputs, options) {
        Ok(file) => file,
        Err(e) => return Copied::from(format!("Could not copy to the clipboard: {e}")),
    };
    if let Err(e) = set_file(clipboard, copy.clone()) {
        return Copied::from(e);
    }
    let name = copy
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
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
