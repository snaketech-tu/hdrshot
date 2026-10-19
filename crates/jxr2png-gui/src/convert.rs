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
            let mut last = None;
            for (index, path) in files {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                send(&tx, Event::Started(index));
                let result = jxr2png_core::convert_file(&path, &options);
                if let Ok(outputs) = &result {
                    // The HDR PNG when there is one: it is the faithful copy.
                    last = outputs.hdr.clone().or_else(|| outputs.sdr.clone());
                }
                send(
                    &tx,
                    Event::Finished(index, result.map_err(|e| e.to_string())),
                );
            }
            let copied = clipboard
                .zip(last)
                .map(|(clipboard, path)| copy(&clipboard, &path));
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

/// Put `path` on the clipboard as a file, so pasting it (into Discord,
/// a browser or a folder) gives the PNG exactly as written, HDR included.
/// Returns the message for the user.
fn copy(clipboard: &SharedClipboard, path: &Path) -> String {
    let mut clipboard = clipboard.lock().unwrap_or_else(|e| e.into_inner());
    let clipboard = match &mut *clipboard {
        Some(c) => c,
        None => match arboard::Clipboard::new() {
            Ok(c) => clipboard.insert(c),
            Err(e) => return format!("Could not copy to the clipboard: {e}"),
        },
    };
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    match clipboard.set().file_list(&[path]) {
        Ok(()) => format!("Copied {name} to the clipboard"),
        Err(e) => format!("Could not copy to the clipboard: {e}"),
    }
}
