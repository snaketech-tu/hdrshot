# Changelog

## [Unreleased]

### Added

- The desktop app shows a summary when a batch finishes: how many images
  were saved, their size, where they went and anything that went wrong,
  with **Open folder** and **Retry failed**.
- While converting, the Convert button fills up as the batch progresses
  and shows the time left.
- Files can be dropped anywhere on the window.
- The window opens where it was last closed, at the same size, and never
  taller than the screen.
- The save menu offers the last three folders used.
- A pill at the top of the window shows the watch folder while it is on;
  clicking it pauses watching.
- While the list is empty, the app offers to watch the Game Bar or NVIDIA
  app screenshot folder, whichever holds the newest HDR screenshot. The
  watch folder menu in Settings lists both when they exist.
- Several rows can be selected with Ctrl-click, Shift-click and Ctrl+A, then
  converted or removed together from the right-click menu or with Delete.
  Ctrl+C copies the selected image for Discord and Ctrl+O adds files.
- **Undo** after removing files or clearing the list, for a few seconds, or
  Ctrl+Z.
- Clicking a thumbnail, or pressing Space, shows the image as large as the
  window, with the previous and next image a click or arrow key away.
- Settings shows the exact names an image gets with the current settings.
- A **Theme** setting: System, Light or Dark.
- The desktop app checks for a newer version by itself once a week; when
  there is one, the button at the bottom right becomes **Update to** and the
  version. **Check for updates once a week** in Settings turns this off.

### Changed

- The desktop app's option to move originals to the Recycle Bin is now
  called **Replace the original**, and the button for the steps after
  converting shows whether originals are kept or replaced.
- The watch folder, in the desktop app and with `--watch`, includes its
  subfolders, so the NVIDIA app's folder (`Videos\NVIDIA`, with a subfolder
  per game) can be watched as a whole, new game folders included.
- The desktop app's settings moved off the main window: a row of buttons
  under the file list sums them up (what to create, where to save, the
  largest size, what happens after converting) and opens a small menu for
  each, and a Settings page, opened from the top right, holds all of them,
  including the former Advanced section. The file list gets most of the
  window, which opens 640 points tall, so it fits 1080p screens at 150%
  scaling.
- A new look for the desktop app: the Inter typeface, drawn icons, larger
  thumbnails, rows that show each image's size and type and, once
  converted, the files made and their sizes, or why it failed, and buttons
  on each row that show when you hover it. Secondary text has more
  contrast, and keyboard focus is outlined.
- **Convert** converts only the files not converted yet; once all are
  converted, **Convert again** converts them all again.

## [2.1.0] - 2026-10-21

### Added

- Watch folder: the desktop app (under Advanced) and `--watch FOLDER`
  convert each new image saved to a folder, such as the Game Bar captures
  folder, as soon as it is completely written. Existing images and PNGs in
  the folder are left alone.
- Convert with hdrshot in the right-click menu of `.jxr`, `.wdp`, `.exr`,
  `.jxl` and `.hdr` files on Windows, added by Open .jxr files with
  hdrshot. It converts with the saved settings; several selected files
  become one batch.
- Move originals to the Recycle Bin (Trash) after converting, opt-in: the
  Originals setting in the app and `--trash-originals`. Only originals
  whose files were all written are moved.
- JPEG as the SDR format: the SDR format setting in the app and
  `--sdr-format jpeg` with `--jpeg-quality` (default 90).
- Releases publish the crates to crates.io and update the winget packages
  from GitHub Actions. Manifests for winget (`SnakeTech.hdrshot`,
  `SnakeTech.hdrshot.CLI`) and a Scoop bucket are in `packaging/`.
- When winget or Scoop installed the desktop app, Check for updates shows
  the `winget upgrade` or `scoop update` command instead of replacing the
  executable.

### Changed

- Updated the JPEG XR decoder (`jxr`) to 0.3. Output is identical on the
  test images and sample captures.
- Building from source needs Rust 1.99 or newer, now declared in the
  crates, so older toolchains get a clear message.
- `hdrshot-egui-backend` 0.2.0 drops the `rayon`, `log` and `raster_stats`
  features, which the app never used.
- The desktop app's window opens 820 points tall, to fit the new settings.

## [2.0.1] - 2026-10-21

### Added

- Discord-compatible PNG option in the desktop app, and `--for-discord` on
  the command line: HDR PNGs are saved without the `cICP` and `mDCV`
  chunks, the way Copy for Discord copies them, for uploading the saved
  files to Discord. Windows Photos and most HDR viewers show such PNGs in
  SDR only, so it is off by default.
- The desktop app's updater checks each download against the release's
  `SHA256SUMS.txt` and refuses one that doesn't match.
- hdrshot is listed under Open with for `.wdp`, `.exr`, `.jxl` and `.hdr`
  files, on Windows once Open .jxr files with hdrshot was used, on macOS
  and on Linux. It doesn't become their default.

### Changed

- With AVIF or Ultra HDR JPEG output, the desktop app makes the Discord PNG
  only while converting the last image of a batch. For the other images,
  Copy for Discord makes it when clicked, so batches finish sooner and use
  less temporary space.
- The desktop app keeps the previews of listed images in less than half the
  memory, about 0.5 MB per image instead of 1 to 1.3 MB.

### Fixed

- Two inputs that would make the same output file, such as `a.jxr` and
  `a.exr`, or two `shot.jxr` from different folders converted into one, no
  longer overwrite each other. On the command line, converting them at the
  same time could also leave a broken PNG. The second one is now not
  converted, with a message naming the other. The same file given twice on
  the command line is converted once.
- Double-clicking a converted image in the desktop app on Windows opens it
  directly instead of through the command prompt, so file names with `&`
  or `%` open correctly.
- `--sdr-white`, `--sdr-peak`, `--hdr-peak` and `--input-white` only accept
  numbers above 0, instead of writing black images for 0.
- Closing the desktop app during a conversion lets the current file finish,
  so no temporary file is left in the folder.
- Files dropped onto the desktop app or pasted with Ctrl+V during a
  conversion are added to the list instead of being ignored.
- Adding OpenEXR files to the desktop app no longer freezes it while the
  whole file is read just for its size.
- Stopping the command line with Ctrl+C no longer leaves temporary files
  next to the images being written.
- The command line's progress line no longer shows `←[2K` in the classic
  Windows console.
- AVIF chroma uses the exact neutral value, so grey areas have no faint
  tint.
- Folder scans no longer loop forever on a link to a parent folder.

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
- A refreshed desktop app look, in colors taken from the icon: the icon
  and tagline in the header, a dashed drop area, joined choice buttons,
  settings grouped on a card, files on rounded rows with status labels,
  and a highlighted Convert button. Dark and light themes both follow the
  system.
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
