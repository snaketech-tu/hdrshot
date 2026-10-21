//! Saves the app's settings between runs.
//!
//! Uses the same file and format as eframe (`app.ron`, a map of RON
//! strings), so settings carry over from versions built with eframe.
//! Settings saved under the app's old name, jxr2png, are read until the
//! first save under the new one.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::Serialize;
use serde::de::DeserializeOwned;

const APP_ID: &str = "hdrshot";
const OLD_APP_ID: &str = "jxr2png";

fn path() -> Option<PathBuf> {
    path_for(APP_ID)
}

fn path_for(app_id: &str) -> Option<PathBuf> {
    let dir = if cfg!(windows) {
        std::env::var_os("APPDATA").map(|p| PathBuf::from(p).join(app_id).join("data"))
    } else if cfg!(target_os = "macos") {
        std::env::home_dir().map(|p| p.join("Library/Application Support").join(app_id))
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| std::env::home_dir().map(|p| p.join(".local/share")))
            .map(|p| p.join(app_id))
    };
    dir.map(|d| d.join("app.ron"))
}

fn read_all() -> HashMap<String, String> {
    let read = |path: Option<PathBuf>| std::fs::read_to_string(path?).ok();
    read(path())
        .or_else(|| read(path_for(OLD_APP_ID)))
        .and_then(|text| ron::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn load<T: DeserializeOwned>(key: &str) -> Option<T> {
    ron::from_str(read_all().get(key)?).ok()
}

pub fn save<T: Serialize>(key: &str, value: &T) {
    let Some(path) = path() else {
        return;
    };
    let mut all = read_all();
    let Ok(value) = ron::to_string(value) else {
        return;
    };
    all.insert(key.to_owned(), value);
    let Ok(text) = ron::ser::to_string_pretty(&all, ron::ser::PrettyConfig::default()) else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, text);
}
