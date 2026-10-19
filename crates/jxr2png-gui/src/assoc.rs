//! Making jxr2png the app that opens `.jxr` files.
//!
//! Windows doesn't let programs take over a file type on their own: the app
//! registers itself as able to open `.jxr`, then the user confirms the
//! choice in the Settings page this opens. On Linux the default is set with
//! `xdg-mime`. On macOS the default comes from the app bundle, so there is
//! nothing to do here.

/// Whether this platform supports setting the default from inside the app.
pub const SUPPORTED: bool = cfg!(any(windows, all(unix, not(target_os = "macos"))));

pub enum Outcome {
    /// jxr2png now opens `.jxr` files.
    Done,
    /// The system's settings were opened for the user to confirm.
    #[cfg_attr(not(windows), allow(dead_code))]
    Confirm,
}

#[cfg(windows)]
mod imp {
    use std::io;

    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    use super::Outcome;

    const PROG_ID: &str = "jxr2png.jxr";
    const CLASSES: &str = r"Software\Classes";
    const CAPABILITIES: &str = r"Software\jxr2png\Capabilities";

    pub fn is_default() -> bool {
        let user = RegKey::predef(HKEY_CURRENT_USER);
        let choice = user
            .open_subkey(
                r"Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\.jxr\UserChoice",
            )
            .and_then(|k| k.get_value::<String, _>("ProgId"));
        match choice {
            Ok(prog_id) => prog_id == PROG_ID,
            // Without a choice made in Windows, the class default applies.
            Err(_) => user
                .open_subkey(format!(r"{CLASSES}\.jxr"))
                .and_then(|k| k.get_value::<String, _>(""))
                .is_ok_and(|v| v == PROG_ID),
        }
    }

    /// Point an existing registration at this copy of the program, in case
    /// it was moved since.
    pub fn refresh() {
        let user = RegKey::predef(HKEY_CURRENT_USER);
        if user.open_subkey(format!(r"{CLASSES}\{PROG_ID}")).is_ok() {
            let _ = register();
        }
    }

    pub fn make_default() -> Result<Outcome, String> {
        register().map_err(|e| format!("Could not register jxr2png: {e}"))?;
        if is_default() {
            return Ok(Outcome::Done);
        }
        // Windows 11 opens jxr2png's own page; Windows 10 the general one.
        std::process::Command::new("explorer")
            .arg("ms-settings:defaultapps?registeredAppUser=jxr2png")
            .spawn()
            .map_err(|e| format!("Could not open Settings: {e}"))?;
        Ok(Outcome::Confirm)
    }

    fn register() -> io::Result<()> {
        let exe = std::env::current_exe()?.display().to_string();
        let user = RegKey::predef(HKEY_CURRENT_USER);

        let (prog, _) = user.create_subkey(format!(r"{CLASSES}\{PROG_ID}"))?;
        prog.set_value("", &"JPEG XR image")?;
        prog.create_subkey("DefaultIcon")?
            .0
            .set_value("", &format!("\"{exe}\",0"))?;
        prog.create_subkey(r"shell\open\command")?
            .0
            .set_value("", &format!("\"{exe}\" \"%1\""))?;

        let (ext, _) = user.create_subkey(format!(r"{CLASSES}\.jxr"))?;
        ext.create_subkey("OpenWithProgids")?
            .0
            .set_value(PROG_ID, &"")?;
        // Become the default where Windows hasn't recorded a choice yet.
        if ext.get_value::<String, _>("").is_err() {
            ext.set_value("", &PROG_ID)?;
        }

        let (caps, _) = user.create_subkey(CAPABILITIES)?;
        caps.set_value("ApplicationName", &"jxr2png")?;
        caps.set_value(
            "ApplicationDescription",
            &"Converts HDR JPEG XR screenshots to PNG",
        )?;
        caps.create_subkey("FileAssociations")?
            .0
            .set_value(".jxr", &PROG_ID)?;
        user.create_subkey(r"Software\RegisteredApplications")?
            .0
            .set_value("jxr2png", &CAPABILITIES)?;

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

    const DESKTOP_FILE: &str = "jxr2png.desktop";
    const MIME_TYPE: &str = "image/vnd.ms-photo";

    pub fn is_default() -> bool {
        Command::new("xdg-mime")
            .args(["query", "default", MIME_TYPE])
            .output()
            .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).trim() == DESKTOP_FILE)
    }

    pub fn refresh() {
        if desktop_file().is_some_and(|p| p.exists()) {
            let _ = write_desktop_file();
        }
    }

    pub fn make_default() -> Result<Outcome, String> {
        write_desktop_file().map_err(|e| format!("Could not install jxr2png.desktop: {e}"))?;
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
        // The icon goes where desktops look up `Icon=jxr2png`.
        if let Some(data) = data_home() {
            let icons = data.join("icons/hicolor/256x256/apps");
            std::fs::create_dir_all(&icons)?;
            std::fs::write(icons.join("jxr2png.png"), crate::ICON_PNG)?;
        }
        std::fs::create_dir_all(path.parent().expect("has parent"))?;
        std::fs::write(
            &path,
            format!(
                "[Desktop Entry]\n\
                 Type=Application\n\
                 Name=jxr2png\n\
                 Comment=Convert HDR JPEG XR screenshots to PNG\n\
                 Icon=jxr2png\n\
                 Exec=\"{exe}\" %F\n\
                 MimeType={MIME_TYPE};\n\
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
