//! Looks for a newer release on GitHub and installs it.
//!
//! On Windows and Linux the new executable replaces the running one, which
//! then starts it and quits. On macOS the new disk image is opened, since
//! an app bundle is replaced by dragging it to Applications.

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde::Deserialize;

const RELEASES_URL: &str = "https://api.github.com/repos/snaketech-tu/hdrshot/releases?per_page=30";

/// The version of this build.
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// Passed to the new executable after an update, so it waits for the old
/// one to quit before it claims the single-instance socket.
pub const UPDATED_FLAG: &str = "--updated";

/// How the release download of the desktop app for this platform ends, as
/// named by `release.yml`. Only the end is matched, so the update still
/// finds it if the app is renamed; GitHub redirects the API of a renamed
/// repository too.
const ASSET_SUFFIX: Option<&str> = if cfg!(windows) {
    Some("-gui.exe")
} else if cfg!(target_os = "macos") {
    Some("-gui-macos.dmg")
} else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
    Some("-gui-linux-x64.tar.gz")
} else {
    None
};

pub struct Release {
    pub version: String,
    pub notes: String,
    /// The release's page on GitHub.
    pub page: String,
    /// The app for this platform, if the release has it.
    pub download: Option<Download>,
}

#[derive(Clone)]
pub struct Download {
    /// The file name, used for the download on macOS and Linux.
    #[cfg_attr(windows, allow(dead_code))]
    pub name: String,
    pub url: String,
}

pub enum Checked {
    UpToDate,
    /// The releases newer than this build, newest first.
    Newer(Vec<Release>),
}

pub enum Installed {
    /// The new version is in place and should be started.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    Restart(PathBuf),
    /// The disk image is open for the user to install from (macOS).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Opened,
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    body: Option<String>,
    html_url: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<ApiAsset>,
}

#[derive(Deserialize)]
struct ApiAsset {
    name: String,
    browser_download_url: String,
}

fn agent() -> ureq::Agent {
    // The system's certificates, so HTTPS also works behind proxies that
    // add their own.
    let tls = ureq::tls::TlsConfig::builder()
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .build();
    ureq::Agent::config_builder()
        .tls_config(tls)
        // Followed by `get`.
        .max_redirects(0)
        .timeout_global(Some(Duration::from_secs(120)))
        .user_agent(format!("hdrshot/{CURRENT}"))
        .build()
        .into()
}

/// GETs `url`, following redirects. ureq follows them itself, but fails
/// on GitHub's redirect from a release download to its storage (the
/// redirect's chunked body is read as broken), so it's done here.
fn get(url: &str, accept: &str) -> Result<ureq::http::Response<ureq::Body>, ureq::Error> {
    let agent = agent();
    let mut url = url.to_owned();
    for _ in 0..5 {
        let response = agent.get(&url).header("Accept", accept).call()?;
        if !response.status().is_redirection() {
            return Ok(response);
        }
        url = response
            .headers()
            .get("location")
            .and_then(|l| l.to_str().ok())
            .filter(|l| l.starts_with("https://"))
            .ok_or(ureq::Error::RedirectFailed)?
            .to_owned();
    }
    Err(ureq::Error::TooManyRedirects)
}

/// Asks GitHub for the published releases.
pub fn check() -> Result<Checked, String> {
    check_since(CURRENT)
}

fn check_since(current: &str) -> Result<Checked, String> {
    let releases: Vec<ApiRelease> = get(RELEASES_URL, "application/vnd.github+json")
        .and_then(|mut response| response.body_mut().read_json())
        .map_err(|e| format!("Couldn't reach GitHub: {e}"))?;
    let current = parse_version(current);
    let mut newer: Vec<_> = releases
        .into_iter()
        .filter(|r| !r.draft && !r.prerelease)
        .filter_map(|r| Some((parse_version(&r.tag_name)?, r)))
        .filter(|(version, _)| Some(*version) > current)
        .collect();
    newer.sort_by_key(|(version, _)| std::cmp::Reverse(*version));
    if newer.is_empty() {
        return Ok(Checked::UpToDate);
    }
    Ok(Checked::Newer(
        newer
            .into_iter()
            .map(|(_, r)| Release {
                version: r.tag_name.trim_start_matches('v').to_owned(),
                notes: r.body.unwrap_or_default(),
                page: r.html_url,
                download: ASSET_SUFFIX.and_then(|suffix| {
                    r.assets
                        .into_iter()
                        .find(|a| a.name.ends_with(suffix))
                        .map(|a| Download {
                            name: a.name,
                            url: a.browser_download_url,
                        })
                }),
            })
            .collect(),
    ))
}

/// `1.2.3` or `v1.2.3` as numbers; `None` for anything else.
fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let mut parts = text.trim().trim_start_matches('v').split('.');
    let mut next = || parts.next()?.parse().ok();
    let version = (next()?, next()?, next()?);
    parts.next().is_none().then_some(version)
}

/// Downloads the new app and puts it in place of this one.
pub fn install(download: &Download) -> Result<Installed, String> {
    install_here(download).map_err(|e| format!("The update failed: {e}"))
}

#[cfg(windows)]
fn install_here(download: &Download) -> io::Result<Installed> {
    let exe = std::env::current_exe()?;
    let new = sibling(&exe, ".new");
    let old = sibling(&exe, ".old");
    fetch(&download.url, &new)?;
    // A running executable can't be overwritten, but it can be renamed.
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old)?;
    if let Err(e) = std::fs::rename(&new, &exe) {
        let _ = std::fs::rename(&old, &exe);
        return Err(e);
    }
    Ok(Installed::Restart(exe))
}

#[cfg(target_os = "macos")]
fn install_here(download: &Download) -> io::Result<Installed> {
    let dir = std::env::home_dir()
        .map(|home| home.join("Downloads"))
        .filter(|dir| dir.is_dir())
        .unwrap_or_else(std::env::temp_dir);
    let dmg = dir.join(&download.name);
    fetch(&download.url, &dmg)?;
    let status = Command::new("open").arg(&dmg).status()?;
    if !status.success() {
        return Err(io::Error::other("couldn't open the disk image"));
    }
    Ok(Installed::Opened)
}

#[cfg(not(any(windows, target_os = "macos")))]
fn install_here(download: &Download) -> io::Result<Installed> {
    let exe = std::env::current_exe()?;
    let work = std::env::temp_dir().join(format!("hdrshot-update-{}", std::process::id()));
    std::fs::create_dir_all(&work)?;
    let result = (|| {
        let archive = work.join(&download.name);
        fetch(&download.url, &archive)?;
        let status = Command::new("tar")
            .arg("-xzf")
            .arg(&archive)
            .arg("-C")
            .arg(&work)
            .status()?;
        if !status.success() {
            return Err(io::Error::other("couldn't unpack the download"));
        }
        // The archive holds one folder named like it, with the app's
        // executable ending in "-gui".
        let folder = work.join(download.name.trim_end_matches(".tar.gz"));
        let unpacked = std::fs::read_dir(&folder)?
            .flatten()
            .map(|entry| entry.path())
            .find(|path| {
                path.is_file()
                    && path
                        .file_name()
                        .is_some_and(|name| name.to_string_lossy().ends_with("-gui"))
            })
            .ok_or_else(|| io::Error::other("the download has no app in it"))?;
        // Copied next to the executable first, so the rename below is
        // atomic; the copy keeps the executable bit.
        let new = sibling(&exe, ".new");
        std::fs::copy(&unpacked, &new)?;
        // The running program keeps its old file open; renaming over it
        // is fine.
        std::fs::rename(&new, &exe).inspect_err(|_| {
            let _ = std::fs::remove_file(&new);
        })
    })();
    let _ = std::fs::remove_dir_all(&work);
    result.map(|()| Installed::Restart(exe))
}

/// `path` with `suffix` added to its file name.
#[cfg_attr(target_os = "macos", allow(dead_code))]
fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_owned();
    name.push(suffix);
    path.with_file_name(name)
}

/// Downloads `url` to the file `to`.
fn fetch(url: &str, to: &Path) -> io::Result<()> {
    let result = (|| {
        let mut response = get(url, "application/octet-stream").map_err(io::Error::other)?;
        let mut file = File::create(to)?;
        io::copy(&mut response.body_mut().as_reader(), &mut file)?;
        file.sync_all()
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(to);
    }
    result
}

/// Starts the updated app, which waits for this one to quit.
pub fn restart(exe: &Path) -> io::Result<()> {
    Command::new(exe).arg(UPDATED_FLAG).spawn().map(drop)
}

/// Deletes the executable an update on Windows left behind.
pub fn clean_up() {
    #[cfg(windows)]
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(sibling(&exe, ".old"));
    }
}

/// A piece of release notes, which are Markdown.
#[derive(Debug, PartialEq)]
pub enum Block {
    Heading(String),
    Item(String),
    Text(String),
}

/// Splits release notes into headings, list items and paragraphs, with
/// inline Markdown (code, emphasis, links) reduced to plain text.
pub fn parse_notes(notes: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut current: Option<Block> = None;
    for line in notes.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            blocks.extend(current.take());
        } else if let Some(heading) = trimmed.strip_prefix('#') {
            blocks.extend(current.take());
            blocks.push(Block::Heading(plain(
                heading.trim_start_matches('#').trim(),
            )));
        } else if let Some(item) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        {
            blocks.extend(current.take());
            current = Some(Block::Item(plain(item)));
        } else {
            match &mut current {
                Some(Block::Item(text) | Block::Text(text)) => {
                    text.push(' ');
                    text.push_str(&plain(trimmed));
                }
                _ => current = Some(Block::Text(plain(trimmed))),
            }
        }
    }
    blocks.extend(current);
    blocks
}

fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        // [label](url) keeps the label.
        if c == '['
            && let Some(close) = rest.find("](")
            && let Some(end) = rest[close..].find(')')
        {
            out.push_str(&rest[1..close]);
            rest = &rest[close + end + 1..];
            continue;
        }
        if !matches!(c, '`' | '*') {
            out.push(c);
        }
        rest = &rest[c.len_utf8()..];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("1.10.0"), Some((1, 10, 0)));
        assert_eq!(parse_version("v1.2"), None);
        assert_eq!(parse_version("v1.2.3.4"), None);
        assert_eq!(parse_version("dev"), None);
        assert!(parse_version("1.10.0") > parse_version("1.9.9"));
        assert!(parse_version(CURRENT).is_some());
    }

    #[test]
    fn notes() {
        let notes = "### Added\n\n- The command line takes `folders`\n  and wildcards.\n\
                     - See [the README](https://example.com) for **more**.\n\nA closing line\n\
                     that wraps.";
        assert_eq!(
            parse_notes(notes),
            vec![
                Block::Heading("Added".into()),
                Block::Item("The command line takes folders and wildcards.".into()),
                Block::Item("See the README for more.".into()),
                Block::Text("A closing line that wraps.".into()),
            ]
        );
    }

    #[test]
    fn unclosed_link_stays() {
        assert_eq!(plain("a [b c"), "a [b c");
    }
}
