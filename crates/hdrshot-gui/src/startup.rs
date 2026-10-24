//! Starting hdrshot in the tray when the user signs in to Windows, through
//! the current user's `Run` key, where Task Manager's Startup apps lists it
//! too.

/// Whether this platform can start hdrshot at sign-in.
pub(crate) const SUPPORTED: bool = cfg!(windows);

/// On the command line of the start at sign-in: start in the tray, with the
/// window hidden.
pub(crate) const TRAY_FLAG: &str = "--tray";

/// Starts hdrshot at sign-in from now on, or stops that.
#[cfg_attr(not(windows), allow(unused_variables))]
pub(crate) fn set(on: bool) -> Result<(), String> {
    #[cfg(windows)]
    return imp::set(on).map_err(|e| format!("Couldn't change Start with Windows: {e}"));
    #[cfg(not(windows))]
    Ok(())
}

/// Points the start at sign-in at this copy of hdrshot, which may have
/// moved since, e.g. to a new version's folder.
pub(crate) fn refresh() {
    #[cfg(windows)]
    let _ = imp::write_command();
}

#[cfg(windows)]
mod imp {
    use std::io;

    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};

    const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    /// Where Task Manager notes the startup apps the user turned off.
    const APPROVED: &str =
        r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
    const VALUE: &str = "hdrshot";

    pub fn set(on: bool) -> io::Result<()> {
        let user = RegKey::predef(HKEY_CURRENT_USER);
        if on {
            write_command()?;
            // Turned off in Task Manager earlier, it would stay off.
            if let Ok(approved) = user.open_subkey_with_flags(APPROVED, KEY_SET_VALUE) {
                let _ = approved.delete_value(VALUE);
            }
            return Ok(());
        }
        match user.open_subkey_with_flags(RUN, KEY_SET_VALUE) {
            Ok(run) => match run.delete_value(VALUE) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
                _ => Ok(()),
            },
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }

    pub fn write_command() -> io::Result<()> {
        let exe = std::env::current_exe()?;
        let (run, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(RUN)?;
        let command = format!("\"{}\" {}", exe.display(), super::TRAY_FLAG);
        run.set_value(VALUE, &command)
    }
}
