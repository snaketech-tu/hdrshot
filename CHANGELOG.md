# Changelog

## Unreleased

### Added

- A "Made with ♥ by snake.tech" line at the bottom of the desktop app,
  linking to Ko-fi.

### Changed

- The desktop app draws on the CPU instead of the GPU and only redraws when
  something changes. It no longer looks like a game to the Nvidia driver,
  and the G-Sync flicker stays fixed without redrawing every frame.
- The Resolution menu shows the size the listed files come out at, such as
  "1080p (2580×1080)" for ultrawide screenshots.

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
