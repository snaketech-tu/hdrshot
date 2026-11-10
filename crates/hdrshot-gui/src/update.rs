//! Looks for a newer release on GitHub and installs it.
//!
//! On Windows and Linux the new executable replaces the running one, which
//! then starts it and quits. On macOS the new disk image is opened, since
//! an app bundle is replaced by dragging it to Applications.

use std::fs::File;
use std::io::{self, Read};
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

/// The release download of the desktop app for this platform, as named by
/// `release.yml`. Releases also carry the Windows app as `hdrshot-gui.exe`
/// for versions up to 2.1.0, which look for a name ending in `-gui.exe`.
const ASSET: Option<&str> = if cfg!(windows) {
    Some("hdrshot.exe")
} else if cfg!(target_os = "macos") {
    Some("hdrshot-gui-macos.dmg")
} else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
    Some("hdrshot-gui-linux-x64.tar.gz")
} else {
    None
};

/// The release's list of SHA-256 checksums, as made by `release.yml` with
/// `sha256sum`.
const CHECKSUMS: &str = "SHA256SUMS.txt";

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
    pub name: String,
    pub url: String,
    /// Where the release's checksums are, if it has them.
    pub checksums: Option<String>,
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
    Ok(newer_releases(releases, current, ASSET))
}

/// The releases newer than `current`, newest first, each with the download
/// named `asset` and the release's checksum list, if it has them.
fn newer_releases(releases: Vec<ApiRelease>, current: &str, asset: Option<&str>) -> Checked {
    let current = parse_version(current);
    let mut newer: Vec<_> = releases
        .into_iter()
        .filter(|r| !r.draft && !r.prerelease)
        .filter_map(|r| Some((parse_version(&r.tag_name)?, r)))
        .filter(|(version, _)| Some(*version) > current)
        .collect();
    newer.sort_by_key(|(version, _)| std::cmp::Reverse(*version));
    if newer.is_empty() {
        return Checked::UpToDate;
    }
    Checked::Newer(
        newer
            .into_iter()
            .map(|(_, r)| Release {
                version: r.tag_name.trim_start_matches('v').to_owned(),
                notes: r.body.unwrap_or_default(),
                page: r.html_url,
                download: asset.and_then(|asset| {
                    let checksums = r
                        .assets
                        .iter()
                        .find(|a| a.name == CHECKSUMS)
                        .map(|a| a.browser_download_url.clone());
                    r.assets
                        .into_iter()
                        .find(|a| a.name == asset)
                        .map(|a| Download {
                            name: a.name,
                            url: a.browser_download_url,
                            checksums,
                        })
                }),
            })
            .collect(),
    )
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
    fetch(download, &new)?;
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
    fetch(download, &dmg)?;
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
        fetch(download, &archive)?;
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

/// Downloads `download` to the file `to` and, when the release lists
/// checksums, makes sure the file matches its entry.
fn fetch(download: &Download, to: &Path) -> io::Result<()> {
    let result = (|| {
        let expected = match &download.checksums {
            Some(url) => {
                let list = get(url, "application/octet-stream")
                    .and_then(|mut response| response.body_mut().read_to_string())
                    .map_err(io::Error::other)?;
                Some(listed_sum(&list, &download.name).ok_or_else(|| {
                    io::Error::other(format!("{} is missing from {CHECKSUMS}", download.name))
                })?)
            }
            None => None,
        };
        let mut response =
            get(&download.url, "application/octet-stream").map_err(io::Error::other)?;
        let mut file = File::create(to)?;
        io::copy(&mut response.body_mut().as_reader(), &mut file)?;
        file.sync_all()?;
        drop(file);
        if let Some(expected) = expected
            && expected != sha256(to)?
        {
            return Err(io::Error::other(format!(
                "the download doesn't match its checksum in {CHECKSUMS}"
            )));
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(to);
    }
    result
}

/// The checksum that a `sha256sum` list gives for the file `name`, in
/// lowercase hex.
fn listed_sum(list: &str, name: &str) -> Option<String> {
    list.lines().find_map(|line| {
        let (sum, file) = line.trim().split_once(' ')?;
        // "*" marks a file read in binary mode.
        let file = file.trim_start().trim_start_matches('*');
        (file == name && sum.len() == 64 && sum.bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| sum.to_ascii_lowercase())
    })
}

/// The SHA-256 of the file at `path`, in lowercase hex.
fn sha256(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut digest = ring::digest::Context::new(&ring::digest::SHA256);
    let mut buffer = vec![0; 1 << 16];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(digest
        .finish()
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// Starts the updated app, which waits for this one to quit.
pub fn restart(exe: &Path) -> io::Result<()> {
    Command::new(exe).arg(UPDATED_FLAG).spawn().map(drop)
}

/// The command that updates this copy, when winget or Scoop installed it.
/// Replacing the executable in place would leave the package manager's
/// record at the old version, so the app shows the command instead.
pub fn package_manager_command() -> Option<&'static str> {
    if !cfg!(windows) {
        return None;
    }
    managed_by(&std::env::current_exe().ok()?)
}

fn managed_by(exe: &Path) -> Option<&'static str> {
    let path = exe.to_string_lossy().to_lowercase().replace('\\', "/");
    if path.contains("/winget/packages/") || path.contains("/winget/links/") {
        Some("winget upgrade SnakeTech.hdrshot")
    } else if path.contains("/scoop/apps/hdrshot/") {
        Some("scoop update hdrshot")
    } else {
        None
    }
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
    fn package_managers() {
        let winget = r"C:\Users\A\AppData\Local\Microsoft\WinGet\Packages\SnakeTech.hdrshot_Microsoft.Winget.Source_8wekyb3d8bbwe\hdrshot.exe";
        assert_eq!(
            managed_by(Path::new(winget)),
            Some("winget upgrade SnakeTech.hdrshot")
        );
        let link = r"C:\Users\A\AppData\Local\Microsoft\WinGet\Links\hdrshot-gui.exe";
        assert_eq!(
            managed_by(Path::new(link)),
            Some("winget upgrade SnakeTech.hdrshot")
        );
        let scoop = r"C:\Users\A\scoop\apps\hdrshot\current\hdrshot-gui.exe";
        assert_eq!(managed_by(Path::new(scoop)), Some("scoop update hdrshot"));
        let download = r"C:\Users\A\Downloads\hdrshot.exe";
        assert_eq!(managed_by(Path::new(download)), None);
    }

    /// A release as GitHub's API lists it, with every file `release.yml`
    /// uploads.
    fn api_release(tag: &str, draft: bool, prerelease: bool) -> serde_json::Value {
        let names = [
            "hdrshot.exe",
            "hdrshot-cli.exe",
            "hdrshot-gui.exe",
            "hdrshot-gui-macos.dmg",
            "hdrshot-cli-macos.tar.gz",
            "hdrshot-gui-linux-x64.tar.gz",
            "hdrshot-cli-linux-x64.tar.gz",
            "SHA256SUMS.txt",
        ];
        serde_json::json!({
            "tag_name": tag,
            "body": format!("Notes for {tag}"),
            "html_url": format!("https://github.com/snaketech-tu/hdrshot/releases/tag/{tag}"),
            "draft": draft,
            "prerelease": prerelease,
            "assets": names.iter().map(|name| serde_json::json!({
                "name": name,
                "browser_download_url": format!("https://github.com/snaketech-tu/hdrshot/releases/download/{tag}/{name}"),
            })).collect::<Vec<_>>(),
        })
    }

    fn releases(list: &[serde_json::Value]) -> Vec<ApiRelease> {
        serde_json::from_value(serde_json::Value::Array(list.to_vec())).unwrap()
    }

    fn newer(list: &[serde_json::Value], current: &str, asset: Option<&str>) -> Vec<Release> {
        match newer_releases(releases(list), current, asset) {
            Checked::Newer(releases) => releases,
            Checked::UpToDate => Vec::new(),
        }
    }

    #[test]
    fn each_platform_gets_its_own_download() {
        let list = [api_release("v9.0.0", false, false)];
        for (asset, name) in [
            (Some("hdrshot.exe"), "hdrshot.exe"),
            (Some("hdrshot-gui-macos.dmg"), "hdrshot-gui-macos.dmg"),
            (
                Some("hdrshot-gui-linux-x64.tar.gz"),
                "hdrshot-gui-linux-x64.tar.gz",
            ),
        ] {
            let found = newer(&list, "2.3.2", asset);
            let download = found[0].download.as_ref().expect("a download");
            assert_eq!(download.name, name);
            assert!(download.url.ends_with(&format!("/v9.0.0/{name}")));
            assert!(
                download
                    .checksums
                    .as_deref()
                    .is_some_and(|url| url.ends_with("/v9.0.0/SHA256SUMS.txt"))
            );
        }
        // Platforms without a build are told about the release, with no
        // download.
        assert!(newer(&list, "2.3.2", None)[0].download.is_none());
    }

    #[test]
    fn this_build_looks_for_a_released_file() {
        // The names the release workflow uploads; renaming one there must
        // not leave the Update button looking for a file that is gone.
        let workflow = include_str!("../../../.github/workflows/release.yml");
        if let Some(asset) = ASSET {
            assert!(workflow.contains(asset), "release.yml doesn't make {asset}");
            assert!(
                newer(&[api_release("v9.0.0", false, false)], "2.3.2", Some(asset))[0]
                    .download
                    .is_some()
            );
        }
        assert!(workflow.contains(CHECKSUMS));
        // Versions up to 2.1.0 update from this name.
        assert!(workflow.contains("hdrshot-gui.exe"));
    }

    #[test]
    fn newer_releases_newest_first() {
        let list = [
            api_release("v2.3.1", false, false),
            api_release("v2.10.0", false, false),
            api_release("v2.4.0", false, false),
            api_release("v3.0.0", true, false),
            api_release("v3.0.0-rc.1", false, true),
            api_release("v2.3.2", false, false),
            api_release("nightly", false, false),
        ];
        let found = newer(&list, "2.3.2", Some("hdrshot.exe"));
        let versions: Vec<_> = found.iter().map(|r| r.version.as_str()).collect();
        assert_eq!(versions, ["2.10.0", "2.4.0"]);
        assert_eq!(found[0].notes, "Notes for v2.10.0");
        assert!(found[0].page.ends_with("/tag/v2.10.0"));
        assert!(matches!(
            newer_releases(releases(&list), "2.10.0", Some("hdrshot.exe")),
            Checked::UpToDate
        ));
    }

    #[test]
    fn release_without_the_file_has_no_download() {
        let mut release = api_release("v9.0.0", false, false);
        release["assets"] = serde_json::json!([]);
        let found = newer(&[release], "2.3.2", Some("hdrshot.exe"));
        assert!(found[0].download.is_none());
    }

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
    fn checksums() {
        let sum = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        let list = format!(
            "{}  hdrshot-cli.exe\n{}  hdrshot.exe\n",
            "0".repeat(64),
            sum.to_uppercase()
        );
        assert_eq!(listed_sum(&list, "hdrshot.exe").as_deref(), Some(sum));
        assert_eq!(
            listed_sum(&format!("{sum} *a.dmg"), "a.dmg").as_deref(),
            Some(sum)
        );
        assert_eq!(listed_sum(&list, "hdrshot"), None);
        assert_eq!(listed_sum("abc  a.dmg", "a.dmg"), None);

        let file = std::env::temp_dir().join(format!("hdrshot-sum-{}", std::process::id()));
        std::fs::write(&file, "abc").unwrap();
        let hashed = sha256(&file);
        std::fs::remove_file(&file).unwrap();
        assert_eq!(hashed.unwrap(), sum);
    }

    #[test]
    fn unclosed_link_stays() {
        assert_eq!(plain("a [b c"), "a [b c");
    }
}
