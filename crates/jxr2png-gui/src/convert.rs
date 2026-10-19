//! Background conversion, so the window stays responsive while files are
//! decoded and encoded.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;

use eframe::egui;
use jxr2png_core::{ConvertOptions, Outputs};

pub enum Event {
    Started(usize),
    Finished(usize, Result<Outputs, String>),
    Done,
}

/// A running batch. Dropping it does not stop the thread; call `cancel`.
pub struct Job {
    pub events: Receiver<Event>,
    cancel: Arc<AtomicBool>,
}

impl Job {
    /// Convert `files` one after another (the core already uses every CPU
    /// core per image). Each event carries the file's index in `files`.
    pub fn start(
        files: Vec<(usize, PathBuf)>,
        options: ConvertOptions,
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
            for (index, path) in files {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                send(&tx, Event::Started(index));
                let result = jxr2png_core::convert_file(&path, &options).map_err(|e| e.to_string());
                send(&tx, Event::Finished(index, result));
            }
            send(&tx, Event::Done);
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
