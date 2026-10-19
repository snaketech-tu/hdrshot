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
    /// With `clipboard`, the last image converted is copied to it as SDR.
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
                let result = convert(&path, &options).map(|(outputs, snapshot)| {
                    last = Some(snapshot);
                    outputs
                });
                send(
                    &tx,
                    Event::Finished(index, result.map_err(|e| e.to_string())),
                );
            }
            // Encoding for the clipboard takes a moment for big images, so
            // it happens here rather than on the UI thread.
            let copied = clipboard
                .zip(last)
                .map(|(clipboard, image)| copy(&clipboard, image));
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

/// The last image's SDR pixels, kept as RGB until the batch is over.
struct Rgb {
    name: String,
    width: usize,
    height: usize,
    rgb: Vec<u8>,
}

/// Copy `image` and describe the result for the user.
fn copy(clipboard: &SharedClipboard, image: Rgb) -> String {
    let mut clipboard = clipboard.lock().unwrap_or_else(|e| e.into_inner());
    let clipboard = match &mut *clipboard {
        Some(c) => c,
        None => match arboard::Clipboard::new() {
            Ok(c) => clipboard.insert(c),
            Err(e) => return format!("Could not copy to the clipboard: {e}"),
        },
    };
    let rgba = image
        .rgb
        .as_chunks::<3>()
        .0
        .iter()
        .flat_map(|&[r, g, b]| [r, g, b, 255])
        .collect::<Vec<u8>>();
    let data = arboard::ImageData {
        width: image.width,
        height: image.height,
        bytes: rgba.into(),
    };
    match clipboard.set_image(data) {
        Ok(()) => format!("Copied {} to the clipboard", image.name),
        Err(e) => format!("Could not copy to the clipboard: {e}"),
    }
}

fn convert(path: &Path, options: &ConvertOptions) -> jxr2png_core::Result<(Outputs, Rgb)> {
    let image = jxr2png_core::decode(&std::fs::read(path)?)?;
    let levels = jxr2png_core::light_levels(&image);
    let outputs = jxr2png_core::convert_image(path, &image, &levels, options)?;
    let rgb = Rgb {
        name: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        width: image.width as usize,
        height: image.height as usize,
        rgb: jxr2png_core::sdr_pixels(&image, &levels, &options.sdr),
    };
    Ok((outputs, rgb))
}
