//! Background conversion, so the window stays responsive while files are
//! decoded and encoded.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread;

use eframe::egui;
use jxr2png_core::{ConvertOptions, Outputs};

pub enum Event {
    Started(usize),
    Finished(usize, Result<Outputs, String>),
    /// The batch is over; carries the outcome of copying the last image
    /// to the clipboard, if that was asked for.
    Done(Option<String>),
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
            // The last PNG written, and whether it is the HDR one.
            let mut last = None;
            for (index, path) in files {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                send(&tx, Event::Started(index));
                let result = jxr2png_core::convert_file(&path, &options);
                if let Ok(outputs) = &result {
                    // The HDR PNG when there is one: it is the faithful copy.
                    last = Some(outputs.clone());
                }
                send(
                    &tx,
                    Event::Finished(index, result.map_err(|e| e.to_string())),
                );
            }
            let copied = clipboard
                .zip(last)
                .map(|(clipboard, outputs)| copy(&clipboard, &outputs));
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

/// Put one of `outputs` on the clipboard as a file, preferring the HDR PNG,
/// so pasting it (into Discord, a browser or a folder) gives the full PNG,
/// HDR included. An HDR PNG is copied without its `cICP` chunk so that it
/// also shows right on Discord. Returns the message for the user.
pub fn copy(clipboard: &SharedClipboard, outputs: &Outputs) -> String {
    // The HDR PNG when there is one: it is the faithful copy.
    let (path, hdr) = match (&outputs.hdr, &outputs.sdr) {
        (Some(hdr), _) => (hdr.as_path(), true),
        (None, Some(sdr)) => (sdr.as_path(), false),
        (None, None) => return "Nothing to copy".into(),
    };
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let path = if hdr {
        match discord_copy(path) {
            Ok(copy) => copy,
            Err(e) => return format!("Could not copy to the clipboard: {e}"),
        }
    } else {
        path.to_path_buf()
    };
    let mut clipboard = clipboard.lock().unwrap_or_else(|e| e.into_inner());
    let clipboard = match &mut *clipboard {
        Some(c) => c,
        None => match arboard::Clipboard::new() {
            Ok(c) => clipboard.insert(c),
            Err(e) => return format!("Could not copy to the clipboard: {e}"),
        },
    };
    match clipboard.set().file_list(&[path]) {
        Ok(()) => format!("Copied {name} to the clipboard"),
        Err(e) => format!("Could not copy to the clipboard: {e}"),
    }
}

/// Write a copy of the HDR PNG without its `cICP` chunk, which Discord
/// mishandles (see [`jxr2png_core::for_discord`]), to a temporary folder
/// that only ever holds the latest copy. It keeps the original's name.
fn discord_copy(path: &Path) -> std::io::Result<PathBuf> {
    let dir = std::env::temp_dir().join("jxr2png-clipboard");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    let copy = dir.join(path.file_name().unwrap_or_default());
    std::fs::write(&copy, jxr2png_core::for_discord(&std::fs::read(path)?))?;
    Ok(copy)
}

/// Run [`copy`] on a background thread (reading and rewriting the PNG takes
/// a moment) and send its message to `messages`.
pub fn copy_in_background(
    clipboard: SharedClipboard,
    outputs: Outputs,
    messages: Sender<String>,
    ctx: egui::Context,
) {
    thread::spawn(move || {
        let _ = messages.send(copy(&clipboard, &outputs));
        ctx.request_repaint();
    });
}
