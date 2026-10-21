# Changelog

## [2.0.0] - 2026-10-21

### Added

- OpenEXR (`.exr`), Radiance (`.hdr`) and HDR PNG (PQ or HLG) inputs, in
  the desktop app and on the command line. The format is detected from the
  file's contents. Folder scans include `.exr` and `.hdr` files but not
  PNGs.
- `--input-white` and the desktop app's **Render white** setting choose the
  brightness of 1.0 in OpenEXR and Radiance files. By default it is the
  file's `whiteLuminance`, or 80 nits.
- JPEG XL (`.jxl`) input, decoded in pure Rust: PQ and HLG files keep their
  brightness, float files are read like OpenEXR, and SDR files give the SDR
  PNG only.
- `--format avif` writes the HDR output as a 10-bit BT.2020 PQ AVIF with
  `clli` and `mdcv` light levels, about a twentieth of the PNG's size.
  `--avif-quality`, `--avif-speed` and `--avif-420` tune it.
- `--format ultrahdr` writes an Ultra HDR JPEG: an SDR JPEG, the same
  picture as the SDR PNG, with a gain map that Chrome, Edge, Android and
  Instagram use to show it in HDR. It carries both Adobe gain map XMP and
  ISO 21496-1 metadata.
- Copying an AVIF or Ultra HDR result for Discord, in the desktop app or
  with `--copy`, copies an HDR PNG made for Discord instead, because
  Discord shows AVIF too bright and Ultra HDR JPEGs in SDR only. The
  desktop app makes that PNG while converting, in a temporary folder, so
  copying is instant.
- The desktop app has an **HDR format** choice (PNG, AVIF or Ultra HDR
  JPEG), and its window title shows the version.

### Changed

- jxr2png is now **hdrshot**, an HDR image tool, since it reads and writes
  more than JPEG XR and PNG. The command is `hdrshot`, the desktop app is
  `hdrshot-gui`, and the downloads are named `hdrshot-gui.exe`,
  `hdrshot.exe`, `hdrshot-gui-macos.dmg` and so on. Version 1.2 finds and
  installs the update under the new name.
- Settings saved by jxr2png are read on first start. On Windows the `.jxr`
  association made by jxr2png now opens hdrshot, and Settings lists it
  once, as hdrshot. On Linux the `jxr2png.desktop` entry is replaced by
  `hdrshot.desktop`, keeping it as the default app for `.jxr` if it was.
  On macOS, delete the old jxr2png app after installing hdrshot.
- The crates are `hdrshot-core`, `hdrshot` and `hdrshot-gui`, with
  crates.io metadata. The vendored CPU renderer is the
  `hdrshot-egui-backend` package.
- New app icon: a picture split between flat SDR and bright HDR. It has
  a simplified version for the smallest sizes, a macOS icon on Apple's
  icon grid, and Linux icons from 16 px to 512 px plus a scalable SVG.

### Fixed

- A conversion that would write its PNG over the input file now fails
  instead.
- Copying a file that is not a PNG to the clipboard no longer alters it.

## [1.2.0] - 2026-10-20

### Added

- A **Check for updates** button in the desktop app shows the release
  notes of a newer version and can install it: on Windows and Linux it
  replaces the app and restarts it, on macOS it opens the new disk image.

### Changed

- Releases offer the desktop app and the command-line tool as separate
  downloads for each platform: bare `.exe` files on Windows, a disk image
  for the macOS app, and `.tar.gz` archives for the macOS command line and
  for Linux. File names no longer contain the version, and a
  `SHA256SUMS.txt` lists the checksums.

## [1.1.0] - 2026-10-20

### Added

- The command line takes folders (`-r` for subfolders too) and expands
  wildcards such as `*.jxr` itself, so they work in cmd and PowerShell.
- `--quiet`, `--json`, a progress line on terminals, and a closing summary
  such as "3 of 20 files failed".
- The desktop app's file list: double-click a converted image to open it,
  right-click a row to open either PNG, show it in its folder, convert it
  again or remove it, plus Clear converted files, Retry failed, keyboard
  shortcuts (Delete, Enter, arrow keys) and pasting copied files with
  Ctrl+V.
- The desktop app runs once per user: files opened while it is already
  running, such as several screenshots opened with "Open with", are added
  to the open window instead of each opening a new one.
- On macOS, jxr2png appears in Finder's "Open With" menu for `.jxr` and
  `.wdp` files, can be made their default app in "Get Info", and accepts
  screenshots dropped onto its Dock icon.
- The desktop app shows conversion progress on its Windows taskbar button
  and, when a batch finishes in the background, flashes the taskbar button
  (bounces the Dock icon on macOS).
- Thumbnails in the desktop app's file list. Hovering one shows a larger
  preview; with Advanced settings changed, it shows the automatic result
  and yours one above the other.
- JXR files with transparency keep it: both PNGs get an alpha channel.
  The always-opaque alpha of Game Bar captures is still dropped.
- An SDR (8-bit) JXR gives only the SDR PNG, with a note saying why,
  instead of an HDR PNG that would only hold the SDR picture.
- Output names: `--name` sets a pattern such as `{name}-{type}`, and
  `--subfolders` (and the app's Advanced settings) put HDR and SDR PNGs in
  `hdr` and `sdr` folders, where the `_hdr`/`_sdr` ending can be left off.
- When a copy for Discord is over its 10 MB upload limit, the desktop app
  offers to copy a smaller version (1440p, or lower if needed). On the
  command line, `--copy` copies the last PNG for Discord and
  `--copy-small` also scales it down when it is too large.
- Shell completions (bash, zsh, fish, PowerShell) and a man page in the
  release archives, and a `.desktop` file with an icon in the Linux one.
- A "Made with ♥ by snake.tech" line at the bottom of the desktop app,
  linking to Ko-fi.

### Changed

- The desktop app draws on the CPU instead of the GPU and only redraws when
  something changes. It no longer looks like a game to the Nvidia driver,
  and the G-Sync flicker stays fixed without redrawing every frame.
- The Resolution menu shows the size the listed files come out at, such as
  "1080p (2580×1080)" for ultrawide screenshots.
- The desktop app no longer replaces PNGs that already exist without
  asking. It asks once per batch: Replace, Keep both or Skip.
- The command line no longer replaces PNGs that already exist. It skips
  them and says so; `--overwrite` replaces them as before.
- The command line converts several files at once, about three times as
  fast on a 4-core PC.
- PNGs get the screenshot's modified date, so folders sorted by date stay
  in capture order. `--no-keep-date` or the app's **Keep the capture date**
  setting turns this off.

### Fixed

- Debug builds of the desktop app no longer stop with a "Dropped
  TexturesDelta" error after the first frame.
- A failed conversion no longer leaves a half-written PNG behind. Files are
  written under a temporary name and renamed when complete.

## [1.0.0] - 2026-10-19

The first release: a desktop app and a command-line tool that turn HDR
JPEG XR (`.jxr`) screenshots from Windows Game Bar and Xbox into PNG.

### Highlights

- **HDR PNG** that looks like the original screenshot: 16-bit Rec.2020 PQ
  with `cICP`, `mDCV` and `cLLI`, plus an ICC profile so Windows Photos,
  Chrome and Edge show it in HDR and the Windows Photo Viewer shows a proper
  SDR picture.
- **SDR PNG** for everything else, tone-mapped so it matches how Windows
  shows the screenshot on an SDR screen, keeping hue and saturation.
- **Desktop app** with drag-and-drop for files and folders, HDR, SDR or
  both, optional output folder, scaling to 2160p, 1440p, 1080p or 720p, and
  a "Copy for Discord" button that pastes into Discord with correct colors.
- **Open .jxr files with jxr2png** on Windows and Linux, so double-clicking
  a screenshot converts it.
- **Command line** for batches, converting in parallel, with optional
  tuning of HDR peak, SDR white and SDR peak.

### Downloads

| Platform | File |
| --- | --- |
| Windows 10/11 (x64) | `jxr2png-v1.0.0-windows-x64.zip` |
| macOS 11+ (Apple silicon and Intel) | `jxr2png-v1.0.0-macos-universal.zip` |
| Linux (x64) | `jxr2png-v1.0.0-linux-x64.tar.gz` |

Each archive contains the desktop app and the `jxr2png` command-line tool.
The builds aren't signed with a paid certificate. On Windows, if
SmartScreen appears, choose More info, then Run anyway. On macOS, if the app
can't be opened, choose Open Anyway in System Settings → Privacy & Security,
or run `xattr -dr com.apple.quarantine jxr2png.app jxr2png`.
