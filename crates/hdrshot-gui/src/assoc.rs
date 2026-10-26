//! Making hdrshot the app that opens `.jxr` files, and listing it under
//! "Open with" for the other formats it reads (`.wdp`, `.exr`, `.jxl`,
//! `.hdr`, `.heic` and `.heif`), without becoming their default. JPEGs are
//! left out: most are SDR photos, which hdrshot doesn't convert. On Windows this also adds
//! "Convert with hdrshot" to these files' right-click menu.
//!
//! Windows doesn't let programs take over a file type on their own: the app
//! registers itself as able to open these types, then the user confirms the
//! choice for `.jxr` in the Settings page this opens. On Linux the default
//! is set with `xdg-mime`. On macOS the app bundle declares the types it
//! opens, and Finder's "Open With" and "Get Info" set the default, so there
//! is nothing to do here.

/// Whether this platform supports setting the default from inside the app.
pub const SUPPORTED: bool = cfg!(any(windows, all(unix, not(target_os = "macos"))));

pub enum Outcome {
    /// hdrshot now opens `.jxr` files.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    Done,
    /// The system's settings were opened for the user to confirm.
    #[cfg_attr(not(windows), allow(dead_code))]
    Confirm,
}

#[cfg(windows)]
mod imp {
    use std::io;

    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};

    use super::Outcome;

    const PROG_ID: &str = "hdrshot.jxr";
    const CLASSES: &str = r"Software\Classes";
    const CAPABILITIES: &str = r"Software\hdrshot\Capabilities";
    /// The registration of versions before the rename. Its file type is kept
    /// and pointed at this program, because Windows remembers the user's
    /// choice by this name and doesn't let programs change that choice.
    const OLD_PROG_ID: &str = "jxr2png.jxr";
    const OLD_APP: &str = r"Software\jxr2png";
    /// The right-click menu entry's key.
    const CONVERT_VERB: &str = "hdrshot.convert";
    /// The other types hdrshot reads, with their file type and its name.
    const OPEN_WITH: [(&str, &str, &str); 6] = [
        (".wdp", PROG_ID, "JPEG XR image"),
        (".exr", "hdrshot.exr", "OpenEXR image"),
        (".jxl", "hdrshot.jxl", "JPEG XL image"),
        (".hdr", "hdrshot.hdr", "Radiance HDR image"),
        (".heic", "hdrshot.heic", "HEIC image"),
        (".heif", "hdrshot.heic", "HEIC image"),
    ];

    pub fn is_default() -> bool {
        let user = RegKey::predef(HKEY_CURRENT_USER);
        let choice = user
            .open_subkey(
                r"Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\.jxr\UserChoice",
            )
            .and_then(|k| k.get_value::<String, _>("ProgId"));
        match choice {
            Ok(prog_id) => prog_id == PROG_ID || prog_id == OLD_PROG_ID,
            // Without a choice made in Windows, the class default applies.
            Err(_) => user
                .open_subkey(format!(r"{CLASSES}\.jxr"))
                .and_then(|k| k.get_value::<String, _>(""))
                .is_ok_and(|v| v == PROG_ID || v == OLD_PROG_ID),
        }
    }

    /// Point an existing registration at this copy of the program, in case
    /// it was moved since.
    pub fn refresh() {
        let user = RegKey::predef(HKEY_CURRENT_USER);
        let registered = |id: &str| user.open_subkey(format!(r"{CLASSES}\{id}")).is_ok();
        if registered(PROG_ID) || registered(OLD_PROG_ID) {
            let _ = register();
        }
    }

    pub fn make_default() -> Result<Outcome, String> {
        register().map_err(|e| format!("Could not register hdrshot: {e}"))?;
        if is_default() {
            return Ok(Outcome::Done);
        }
        // Windows 11 opens hdrshot's own page; Windows 10 the general one.
        std::process::Command::new("explorer")
            .arg("ms-settings:defaultapps?registeredAppUser=hdrshot")
            .spawn()
            .map_err(|e| format!("Could not open Settings: {e}"))?;
        Ok(Outcome::Confirm)
    }

    fn register() -> io::Result<()> {
        let exe = std::env::current_exe()?.display().to_string();
        let user = RegKey::predef(HKEY_CURRENT_USER);

        let set_command = |prog_id: &str, name: &str| -> io::Result<()> {
            let (prog, _) = user.create_subkey(format!(r"{CLASSES}\{prog_id}"))?;
            prog.set_value("", &name)?;
            prog.create_subkey("DefaultIcon")?
                .0
                .set_value("", &format!("\"{exe}\",0"))?;
            prog.create_subkey(r"shell\open\command")?
                .0
                .set_value("", &format!("\"{exe}\" \"%1\""))
        };
        set_command(PROG_ID, "JPEG XR image")?;
        if user
            .open_subkey(format!(r"{CLASSES}\{OLD_PROG_ID}"))
            .is_ok()
        {
            set_command(OLD_PROG_ID, "JPEG XR image")?;
            // Listed once in Settings, under the new name.
            let _ = user
                .open_subkey_with_flags(r"Software\RegisteredApplications", KEY_SET_VALUE)
                .and_then(|k| k.delete_value("jxr2png"));
            let _ = user.delete_subkey_all(OLD_APP);
        }

        let (ext, _) = user.create_subkey(format!(r"{CLASSES}\.jxr"))?;
        ext.create_subkey("OpenWithProgids")?
            .0
            .set_value(PROG_ID, &"")?;
        // Become the default where Windows hasn't recorded a choice yet.
        if ext.get_value::<String, _>("").is_err() {
            ext.set_value("", &PROG_ID)?;
        }
        // The other types only get hdrshot under "Open with".
        for (ext, prog_id, name) in OPEN_WITH {
            set_command(prog_id, name)?;
            user.create_subkey(format!(r"{CLASSES}\{ext}\OpenWithProgids"))?
                .0
                .set_value(prog_id, &"")?;
        }

        // "Convert with hdrshot" in the right-click menu of every type, which
        // converts with the saved settings. On Windows 11 it is under "Show
        // more options".
        for ext in std::iter::once(".jxr").chain(OPEN_WITH.map(|(ext, ..)| ext)) {
            let (verb, _) = user.create_subkey(format!(
                r"{CLASSES}\SystemFileAssociations\{ext}\shell\{CONVERT_VERB}"
            ))?;
            verb.set_value("", &"Convert with hdrshot")?;
            verb.set_value("Icon", &format!("\"{exe}\",0"))?;
            // One launch per selected file, with no limit on how many; the
            // running window gathers them into one batch.
            verb.set_value("MultiSelectModel", &"Player")?;
            verb.create_subkey("command")?.0.set_value(
                "",
                &format!("\"{exe}\" {} \"%1\"", crate::single::CONVERT_FLAG),
            )?;
        }

        let (caps, _) = user.create_subkey(CAPABILITIES)?;
        caps.set_value("ApplicationName", &"hdrshot")?;
        caps.set_value("ApplicationDescription", &"HDR image tool")?;
        let (associations, _) = caps.create_subkey("FileAssociations")?;
        associations.set_value(".jxr", &PROG_ID)?;
        for (ext, prog_id, _) in OPEN_WITH {
            associations.set_value(ext, &prog_id)?;
        }
        user.create_subkey(r"Software\RegisteredApplications")?
            .0
            .set_value("hdrshot", &CAPABILITIES)?;

        // SAFETY: plain notification call with no pointers to keep alive.
        unsafe {
            use windows_sys::Win32::UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify};
            SHChangeNotify(
                SHCNE_ASSOCCHANGED as i32,
                SHCNF_IDLIST,
                std::ptr::null(),
                std::ptr::null(),
            );
        }
        Ok(())
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod imp {
    use std::path::PathBuf;
    use std::process::Command;

    use super::Outcome;

    const DESKTOP_FILE: &str = "hdrshot.desktop";
    /// The file of versions before the rename, replaced by the new one.
    const OLD_DESKTOP_FILE: &str = "jxr2png.desktop";
    const MIME_TYPE: &str = "image/vnd.ms-photo";
    /// Listed in the desktop file, so hdrshot is under "Open with" for
    /// them; only `MIME_TYPE` is made the default.
    const MIME_TYPES: &str = "image/vnd.ms-photo;image/jxr;image/x-exr;image/jxl;image/vnd.radiance;image/heic;image/heif;";

    fn default_app() -> Option<String> {
        Command::new("xdg-mime")
            .args(["query", "default", MIME_TYPE])
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
    }

    pub fn is_default() -> bool {
        default_app().is_some_and(|app| app == DESKTOP_FILE || app == OLD_DESKTOP_FILE)
    }

    pub fn refresh() {
        let old = data_home().map(|d| d.join("applications").join(OLD_DESKTOP_FILE));
        if let Some(old) = old.filter(|p| p.exists()) {
            if write_desktop_file().is_err() {
                return;
            }
            if default_app().is_some_and(|app| app == OLD_DESKTOP_FILE) {
                let _ = Command::new("xdg-mime")
                    .args(["default", DESKTOP_FILE, MIME_TYPE])
                    .status();
            }
            let _ = std::fs::remove_file(old);
            if let Some(data) = data_home() {
                let _ = std::fs::remove_file(data.join("icons/hicolor/256x256/apps/jxr2png.png"));
            }
        } else if desktop_file().is_some_and(|p| p.exists()) {
            let _ = write_desktop_file();
        }
    }

    pub fn make_default() -> Result<Outcome, String> {
        write_desktop_file().map_err(|e| format!("Could not install hdrshot.desktop: {e}"))?;
        let status = Command::new("xdg-mime")
            .args(["default", DESKTOP_FILE, MIME_TYPE])
            .status()
            .map_err(|e| format!("Could not run xdg-mime: {e}"))?;
        if !status.success() {
            return Err("xdg-mime could not set the default app.".into());
        }
        Ok(Outcome::Done)
    }

    fn data_home() -> Option<PathBuf> {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
    }

    fn desktop_file() -> Option<PathBuf> {
        Some(data_home()?.join("applications").join(DESKTOP_FILE))
    }

    fn write_desktop_file() -> std::io::Result<()> {
        let path = desktop_file().ok_or(std::io::ErrorKind::NotFound)?;
        let exe = std::env::current_exe()?;
        // Quoted as the Desktop Entry spec requires.
        let exe = exe
            .display()
            .to_string()
            .replace('\\', "\\\\\\\\")
            .replace('"', "\\\\\"")
            .replace('`', "\\\\`")
            .replace('$', "\\\\$");
        // The icon goes where desktops look up `Icon=hdrshot`.
        if let Some(data) = data_home() {
            let icons = data.join("icons/hicolor/256x256/apps");
            std::fs::create_dir_all(&icons)?;
            std::fs::write(icons.join("hdrshot.png"), crate::ICON_PNG)?;
        }
        std::fs::create_dir_all(path.parent().expect("has parent"))?;
        std::fs::write(
            &path,
            format!(
                "[Desktop Entry]\n\
                 Type=Application\n\
                 Name=hdrshot\n\
                 Comment=HDR image tool\n\
                 Icon=hdrshot\n\
                 Exec=\"{exe}\" %F\n\
                 MimeType={MIME_TYPES}\n\
                 Categories=Graphics;\n\
                 Terminal=false\n"
            ),
        )?;
        let _ = Command::new("update-desktop-database")
            .arg(path.parent().expect("has parent"))
            .status();
        Ok(())
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::Outcome;

    pub fn is_default() -> bool {
        false
    }

    pub fn refresh() {}

    pub fn make_default() -> Result<Outcome, String> {
        Err("Not supported on macOS.".into())
    }
}

pub use imp::{is_default, make_default, refresh};
