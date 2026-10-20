# Changelog

## Unreleased

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
