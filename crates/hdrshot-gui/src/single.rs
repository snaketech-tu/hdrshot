//! Keeps one window per user: files opened while the app is already running
//! (e.g. several screenshots picked with "Open with", which starts one
//! process per file) go to the open window instead of opening another.
//!
//! The first instance listens on a local socket (a named pipe on Windows).
//! Later ones send it their file paths, one per line, and exit.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::mpsc::Sender;

use interprocess::local_socket::{
    GenericNamespaced, ListenerOptions, Name, Stream, ToNsName, prelude::*,
};

pub enum Claim {
    /// No other instance runs; this one now receives the files of later
    /// ones.
    First(Listener),
    /// The files went to the instance that was already running.
    HandedOff,
}

pub struct Listener(interprocess::local_socket::Listener);

/// Before the files on the command line, as Explorer's "Convert with
/// hdrshot" passes them: convert them right away. Sent to a running
/// instance as the first line.
pub const CONVERT_FLAG: &str = "--convert";

/// Hands `files` to a running instance if there is one, or else becomes the
/// instance that later ones hand theirs to. `None` when the socket can't be
/// used at all, in which case every launch simply opens its own window.
pub fn claim(files: &[PathBuf], convert: bool) -> Option<Claim> {
    let name = name()?;
    if send(&name, files, convert) {
        return Some(Claim::HandedOff);
    }
    match ListenerOptions::new()
        .name(name.borrow())
        // A socket file left behind by a crash would block the name.
        .try_overwrite(true)
        .create_sync()
    {
        Ok(listener) => Some(Claim::First(Listener(listener))),
        // Another instance started at the same moment and won.
        Err(_) if send(&name, files, convert) => Some(Claim::HandedOff),
        Err(_) => None,
    }
}

impl Listener {
    /// Sends the file lists of later instances to `opened`, or each file
    /// to `convert` when they came with [`CONVERT_FLAG`], from a background
    /// thread. An empty list means the app was started again without
    /// files; either way the window should come to the front.
    pub fn serve(self, opened: Sender<Vec<PathBuf>>, convert: Sender<PathBuf>, ctx: egui::Context) {
        std::thread::spawn(move || {
            for stream in self.0.incoming().flatten() {
                let mut lines: Vec<String> = BufReader::new(stream)
                    .lines()
                    .map_while(Result::ok)
                    .filter(|line| !line.is_empty())
                    .collect();
                let to_convert = lines.first().is_some_and(|l| l == CONVERT_FLAG);
                if to_convert {
                    lines.remove(0);
                }
                let files: Vec<PathBuf> = lines.into_iter().map(PathBuf::from).collect();
                let sent = if to_convert {
                    let all = files.into_iter().all(|f| convert.send(f).is_ok());
                    all && opened.send(Vec::new()).is_ok()
                } else {
                    opened.send(files).is_ok()
                };
                if !sent {
                    break;
                }
                ctx.request_repaint();
            }
        });
    }
}

/// Waits up to `timeout` for the running instance to quit, as the old
/// version does right after starting the new one in an update.
pub fn wait_for_exit(timeout: std::time::Duration) {
    let Some(name) = name() else { return };
    let start = std::time::Instant::now();
    while start.elapsed() < timeout && Stream::connect(name.borrow()).is_ok() {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// Sends `files` to a running instance, to convert right away with
/// `convert`; false if none answers.
fn send(name: &Name<'_>, files: &[PathBuf], convert: bool) -> bool {
    let Ok(mut stream) = Stream::connect(name.borrow()) else {
        return false;
    };
    let mut message = String::new();
    if convert {
        message.push_str(CONVERT_FLAG);
        message.push('\n');
    }
    for file in files {
        // The running instance has its own working directory.
        let file = std::path::absolute(file).unwrap_or_else(|_| file.clone());
        message.push_str(&file.to_string_lossy());
        message.push('\n');
    }
    stream.write_all(message.as_bytes()).is_ok()
}

/// One name per user, since named pipes and Linux's abstract sockets are
/// shared by everyone on the machine.
fn name() -> Option<Name<'static>> {
    let user = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_default();
    let user: String = user
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .collect();
    format!("hdrshot-{user}.sock")
        .to_ns_name::<GenericNamespaced>()
        .ok()
        .map(Name::into_owned)
}
