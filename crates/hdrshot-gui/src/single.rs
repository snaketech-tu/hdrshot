//! Keeps one window per user: files opened while the app is already running
//! (e.g. several screenshots picked with "Open with", which starts one
//! process per file) go to the open window instead of opening another.
//!
//! The first instance listens on a local socket (a named pipe on Windows).
//! Later ones send it their file paths, one per line, and exit.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::mpsc::Sender;

use crate::startup::TRAY_FLAG;
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

/// How hdrshot was started.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Launch {
    /// To open the files on the command line, if any.
    Open,
    /// With [`CONVERT_FLAG`], to convert them right away.
    Convert,
    /// With [`TRAY_FLAG`], at sign-in, to run in the tray.
    Tray,
}

/// Hands `files` to a running instance if there is one, or else becomes the
/// instance that later ones hand theirs to. `None` when the socket can't be
/// used at all, in which case every launch simply opens its own window.
pub fn claim(files: &[PathBuf], launch: Launch) -> Option<Claim> {
    let name = name()?;
    if send(&name, files, launch) {
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
        Err(_) if send(&name, files, launch) => Some(Claim::HandedOff),
        Err(_) => None,
    }
}

impl Listener {
    /// Sends the file lists of later instances to `opened`, or each file
    /// to `convert` when they came with [`CONVERT_FLAG`], from a background
    /// thread. An empty list means the app was started again without
    /// files; either way the window should come to the front. A start in
    /// the tray, with [`TRAY_FLAG`], changes nothing.
    pub fn serve(self, opened: Sender<Vec<PathBuf>>, convert: Sender<PathBuf>, ctx: egui::Context) {
        std::thread::spawn(move || {
            for stream in self.0.incoming().flatten() {
                let lines = BufReader::new(stream).lines().map_while(Result::ok);
                let (launch, files) = parse(lines);
                let sent = match launch {
                    Launch::Tray => continue,
                    Launch::Convert => {
                        let all = files.into_iter().all(|f| convert.send(f).is_ok());
                        all && opened.send(Vec::new()).is_ok()
                    }
                    Launch::Open => opened.send(files).is_ok(),
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

/// Sends `files` to a running instance, with how this one was started;
/// false if none answers.
fn send(name: &Name<'_>, files: &[PathBuf], launch: Launch) -> bool {
    let Ok(mut stream) = Stream::connect(name.borrow()) else {
        return false;
    };
    stream.write_all(message(files, launch).as_bytes()).is_ok()
}

/// What a later instance sends: how it was started, if not plainly, then
/// one file per line.
fn message(files: &[PathBuf], launch: Launch) -> String {
    let mut message = String::new();
    match launch {
        Launch::Open => {}
        Launch::Convert => message.push_str(CONVERT_FLAG),
        Launch::Tray => message.push_str(TRAY_FLAG),
    }
    if launch != Launch::Open {
        message.push('\n');
    }
    for file in files {
        // The running instance has its own working directory.
        let file = std::path::absolute(file).unwrap_or_else(|_| file.clone());
        message.push_str(&file.to_string_lossy());
        message.push('\n');
    }
    message
}

/// How a later instance was started and the files it sent, from the lines
/// of its [`message`].
fn parse(lines: impl Iterator<Item = String>) -> (Launch, Vec<PathBuf>) {
    let mut lines = lines.filter(|line| !line.is_empty()).peekable();
    let launch = match lines.peek().map(String::as_str) {
        Some(CONVERT_FLAG) => Launch::Convert,
        Some(TRAY_FLAG) => Launch::Tray,
        _ => Launch::Open,
    };
    if launch != Launch::Open {
        lines.next();
    }
    (launch, lines.map(PathBuf::from).collect())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_say_how_hdrshot_was_started() {
        let file = std::path::absolute("shot.jxr").unwrap();
        let round_trip =
            |files: &[PathBuf], launch| parse(message(files, launch).lines().map(str::to_owned));
        let files = vec![file.clone()];
        assert_eq!(
            round_trip(&files, Launch::Open),
            (Launch::Open, files.clone())
        );
        assert_eq!(
            round_trip(&files, Launch::Convert),
            (Launch::Convert, files)
        );
        assert_eq!(round_trip(&[], Launch::Tray), (Launch::Tray, vec![]));
        // Started again without files: the window comes to the front.
        assert_eq!(message(&[], Launch::Open), "");
        assert_eq!(round_trip(&[], Launch::Open), (Launch::Open, vec![]));
    }
}
