<div align="center">

<img src="https://raw.githubusercontent.com/snaketech-tu/hdrshot/main/crates/hdrshot-gui/assets/icon.png" width="128" alt="hdrshot icon">

# hdrshot

**HDR image tool. Converts HDR screenshots and images to HDR PNG, AVIF,
Ultra HDR JPEG and SDR PNG.**

[![Release](https://img.shields.io/github/v/release/snaketech-tu/hdrshot?style=flat-square&color=0e7fc0)](https://github.com/snaketech-tu/hdrshot/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/snaketech-tu/hdrshot/total?style=flat-square&color=0e7fc0&cacheSeconds=3600)](https://github.com/snaketech-tu/hdrshot/releases)
[![CI](https://img.shields.io/github/actions/workflow/status/snaketech-tu/hdrshot/ci.yml?branch=main&style=flat-square&label=CI)](https://github.com/snaketech-tu/hdrshot/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/hdrshot?style=flat-square&color=0e7fc0)](https://crates.io/crates/hdrshot)
[![License](https://img.shields.io/github/license/snaketech-tu/hdrshot?style=flat-square&cacheSeconds=3600)](https://github.com/snaketech-tu/hdrshot/blob/main/LICENSE)
![Platforms](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-555?style=flat-square)

[**Download**](https://github.com/snaketech-tu/hdrshot/releases/latest) ·
[Desktop app](#desktop-app) ·
[Command line](#command-line) ·
[How it works](#how-it-works)

<img src="https://raw.githubusercontent.com/snaketech-tu/hdrshot/main/docs/images/app.png" width="560" alt="The hdrshot desktop app after converting a batch of screenshots">

</div>

Windows Game Bar, the NVIDIA app and Xbox save HDR screenshots as JPEG XR,
a format few applications can open. hdrshot converts them, and other HDR images, to
formats that can be viewed and shared: HDR PNG, AVIF or Ultra HDR JPEG for
screens and apps that support HDR, and SDR PNG for everything else, without
changing how the picture looks.

hdrshot was called jxr2png up to version 1.2. Settings and the `.jxr` file
association carry over, and version 1.2 updates to it from **Check for
updates**.

## Features

- **HDR PNG.** Rec.2020 PQ with the brightness of the original
  screenshot, shown in HDR by Windows Photos, Chrome, Edge and HDR image
  viewers.
- **HDR AVIF.** Optionally the same HDR picture as a 10-bit AVIF, about a
  twentieth of the PNG's size, shown in HDR by Chrome, Edge and Firefox.
- **Ultra HDR JPEG.** Or as a JPEG with a gain map: every app shows the
  SDR picture, and Chrome, Edge, Android and Instagram brighten it to HDR.
- **SDR PNG or JPEG.** Tone-mapped the way Windows shows the
  screenshot on an SDR screen, with highlights that roll off smoothly and
  colors that keep their hue.
- **Faithful by default.** Brightness, peak and white level all
  come from the screenshot itself. Tuning options are available but
  not required.
- **Drag and drop.** Drop files or whole folders, pick HDR, SDR or both,
  press Convert.
- **Copy for Discord.** One click puts the image on the clipboard in a
  form Discord shows with correct colors.
- **Other HDR inputs.** Also reads OpenEXR (`.exr`), Radiance (`.hdr`),
  HDR PNG and JPEG XL (`.jxl`) files, for game photo modes, renders and re-tone-mapping.
- **Watch folder.** Optionally converts each new screenshot saved to a
  folder, such as the Game Bar or NVIDIA app captures folder, as soon as it
  is written.
- **Double-click or right-click to convert.** Optionally opens `.jxr` files
  with hdrshot on Windows and Linux, is listed under **Open with** for the
  other formats it reads, and on Windows adds **Convert with hdrshot** to
  their right-click menu.
- **Parallel conversion.** Uses all CPU cores, written in pure Rust with no
  external libraries.

## Download

The desktop app and the `hdrshot` command-line tool are separate downloads.
These links always point to the
[latest release](https://github.com/snaketech-tu/hdrshot/releases/latest).

| Platform | Desktop app | Command line |
| --- | --- | --- |
| Windows 10/11 (x64) | [`hdrshot-gui.exe`](https://github.com/snaketech-tu/hdrshot/releases/latest/download/hdrshot-gui.exe) | [`hdrshot.exe`](https://github.com/snaketech-tu/hdrshot/releases/latest/download/hdrshot.exe) |
| macOS 11+ (Apple silicon and Intel) | [`hdrshot-gui-macos.dmg`](https://github.com/snaketech-tu/hdrshot/releases/latest/download/hdrshot-gui-macos.dmg) | [`hdrshot-cli-macos.tar.gz`](https://github.com/snaketech-tu/hdrshot/releases/latest/download/hdrshot-cli-macos.tar.gz) |
| Linux (x64) | [`hdrshot-gui-linux-x64.tar.gz`](https://github.com/snaketech-tu/hdrshot/releases/latest/download/hdrshot-gui-linux-x64.tar.gz) | [`hdrshot-cli-linux-x64.tar.gz`](https://github.com/snaketech-tu/hdrshot/releases/latest/download/hdrshot-cli-linux-x64.tar.gz) |

`SHA256SUMS.txt` in each release lists the checksums of all downloads. The
app's **Update** button checks its download against this list.

No installer is needed. On Windows, run the `.exe` directly. On macOS, open
the disk image and drag hdrshot to Applications. On Linux and for the macOS
command line, unpack the archive. The builds aren't signed
with a paid certificate, so the first launch needs one extra step:

- **Windows**: if SmartScreen says "Windows protected your PC", choose
  **More info**, then **Run anyway**.
- **macOS**: if the app "can't be opened", open **System Settings → Privacy
  & Security** and choose **Open Anyway** next to hdrshot (on older macOS,
  right-click the app and choose **Open**). Or remove the download
  quarantine in Terminal:
  `xattr -dr com.apple.quarantine /Applications/hdrshot.app`,
  or `xattr -d com.apple.quarantine hdrshot` for the command-line tool.

On Linux, to list the app in your application menu and open `.jxr` files
with it, install the files from the unpacked folder (and copy `hdrshot`
from the command-line archive to `~/.local/bin` as well, if you want it):

```sh
install -Dm755 hdrshot-gui -t ~/.local/bin
install -Dm644 hdrshot.desktop -t ~/.local/share/applications
cp -r icons/hicolor ~/.local/share/icons/
```

### With Cargo

With [Rust](https://rustup.rs) 1.99 or newer installed, both tools can also be built from
[crates.io](https://crates.io/crates/hdrshot):

```sh
cargo install hdrshot        # command-line tool
cargo install hdrshot-gui    # desktop app
```

## Desktop app

1. Drop `.jxr` files or folders anywhere on the window, click the drop
   area or **Add files** to choose files, or paste files copied in Explorer
   or Finder with Ctrl+V.
   On Windows and Linux, files dropped onto the program's icon are added
   too. Files opened while the app is running, for example several
   screenshots picked with **Open with**, go to the window that is already
   open.
2. Check the row of buttons under the list, which sums up the settings:
   what to create (for example **HDR PNG + SDR PNG**), where to save, the
   largest size, and what happens after converting. Click one to change it:
   **HDR + SDR**, **HDR only** or **SDR only**, the HDR format (**PNG**,
   **AVIF** or **Ultra HDR JPEG**), the SDR format (**PNG** or **JPEG**),
   and whether to save next to each image, into one of the last three
   folders used, or into a new one.
3. Press **Convert**. It converts the files not converted yet and shows
   the progress and the time left. When it is done, a summary above the
   settings shows how many images were saved, their size and anything that
   went wrong, with **Open folder** and **Retry failed**.

All settings are also on the **Settings** page, opened with the button at
the top right; Esc or the back button returns to the list. The window opens
where it was last closed, at the same size.

<details>
<summary>More options</summary>

- **Copy for Discord.** After converting, the last PNG (the HDR one if made)
  is copied to the clipboard as a file, so pasting it into Discord, a
  browser upload or a folder gives the full file. Each converted image also
  has its own **Copy for Discord** button. The copy leaves out the `cICP`
  chunk, which makes Discord turn HDR images yellow; the saved file keeps it
  for Windows Photos and HDR viewers. When the copy is larger than
  Discord's 10 MB upload limit, the app offers **Copy a smaller version**,
  which copies it at 1440p, or 1080p or 720p if needed, so it fits. The
  saved PNGs stay at full size. The button shows when you hover a row, and
  stays on the image converted last. With AVIF or Ultra HDR JPEG as the HDR
  format, the copy is an HDR PNG made for Discord, since Discord shows
  AVIF too bright and Ultra HDR JPEGs in SDR only. For the last image of a
  batch, which is copied automatically, the app makes it while converting;
  for the others, **Copy for Discord** makes it, which takes a second or
  two.
- **Discord-compatible PNG**, below the HDR format, saves HDR PNGs without
  the `cICP` and `mDCV` chunks, like the Discord copy, for uploading the
  saved files to Discord yourself. Windows Photos and most HDR viewers then
  show them in SDR only, so it is off by default.
- **SDR format** **JPEG** saves the SDR image as a JPEG at quality 90,
  a fraction of the PNG's size, for chats and forums that don't show HDR.
  JPEG has no transparency, so transparent areas are shown over black.
- **Originals: Move to the Recycle Bin after converting** (the Trash on
  macOS and Linux) clears out the originals once all of their files are
  saved. Images that fail, or whose files already existed, are kept, and so
  is everything when you stop a batch. It is off by default.
- **Watch folder**, in Settings, converts each image saved to
  the chosen folder or its subfolders from then on, with the current
  settings, as soon as it is completely written. On Windows its menu offers
  the Game Bar captures folder (`Videos\Captures`) and the NVIDIA app's
  (`Videos\NVIDIA`, with a subfolder per game) when they exist. Images
  already in the folder are left alone, and PNGs are ignored, since hdrshot
  writes its own there. It works while
  the app is open, and resumes when the app starts again. While it is on, a
  pill at the top of the window names the folder; click it to pause
  watching until you click it again or restart the app. With **Copy the
  last converted image** on, each new screenshot is on the clipboard, ready
  to paste into Discord. While the list is empty, the app offers to turn it
  on for the Game Bar or NVIDIA app folder, whichever holds the newest HDR
  screenshot.
- **Previews.** Each file gets a thumbnail; hover it for a larger SDR
  preview. When the fine-tuning settings are changed, the preview shows the
  automatic result and the result with your settings, so you can compare.
  Click a thumbnail, or select a row and press Space, to see the image as
  large as the window; the arrow keys and the buttons at the sides show the
  previous and next image, and Escape, Space or a click beside the image
  closes it. It is made when you open it and freed when you close it.
- **The file list.** Each row shows a thumbnail, the image's size and type,
  and once converted the files made and their sizes, or why it failed.
  Hovering a row shows buttons to open the image, show it in its folder and
  remove it. Double-click a converted image to open it. Right-click a row to
  open the HDR or SDR image, show it in its folder, convert it again or
  remove it. **Clear**, above the list, removes all files or only the
  converted ones. After removing, **Undo** puts the files back for a few
  seconds. Once every file is converted, the button turns into
  **Convert again**, which converts them all again, for example after
  changing the settings.
- **Selecting rows.** Click a row to select it, Ctrl-click to add or take
  out more, Shift-click to select a range, and press Ctrl+A to select all
  (Cmd on macOS). Right-click a selection to convert or remove all of it.
  The keyboard shortcuts:

  | Key | Does |
  |---|---|
  | Delete | Removes the selected rows |
  | Ctrl+Z | Puts removed rows back |
  | Enter | Converts the files not converted yet |
  | Space | Shows the selected image as large as the window |
  | Ctrl+C | Copies the selected image for Discord |
  | Ctrl+O | Adds files |
  | Ctrl+V | Adds files copied in Explorer or Finder |
  | Arrow keys | Move the selection |
  | Escape | Clears the selection, or closes Settings and the large preview |
- **PNGs that already exist.** When some of the PNGs a batch would make
  are already there, the app asks once per batch whether to replace them,
  keep both (the new ones get a number, e.g. `shot_hdr (2).png`) or skip
  them.
- **Resolution** scales the output down to 2160p, 1440p, 1080p or 720p,
  keeping the aspect ratio and never scaling up. It starts at Source every
  time.
- **Open .jxr files with hdrshot** (Windows and Linux), in Settings, makes
  a double-click on a screenshot open it in the app. Windows asks you to
  confirm the choice in Settings. It also lists hdrshot under **Open with**
  for `.wdp`, `.exr`, `.jxl` and `.hdr` files, without making it their
  default.
  On Windows it also adds **Convert with hdrshot** to the right-click menu
  of these files (on Windows 11 under **Show more options**), which opens
  the app and converts them right away with your saved settings. Selecting
  several files converts them as one batch.
  On macOS, hdrshot is listed under **Open With** for all of these; to make
  it the default for a type, choose it in a file's **Get Info** window and
  click **Change All**.
- **Settings** also has **Keep the capture date** (on by default), which gives
  each PNG the date of its screenshot so folders sorted by date stay in
  capture order. **Save HDR and SDR images in subfolders** writes them
  to `hdr` and `sdr` subfolders, and with that on, **End file names in _hdr
  and _sdr** can be turned off. Below these, it shows the exact names an
  image gets with the current settings, using the first listed file.
  **Theme** picks light or dark, or follows the system (the default).
  Under **Fine-tuning** it holds the tuning
  options from the command line. They are off by default and reset every
  time the app starts, so results always match the original screenshot
  unless you change them; while they are changed, a **Custom tuning**
  button shows under the list.
- **Check for updates**, at the bottom right, looks for a newer release on
  GitHub and shows its release notes. **Update** downloads it and checks it
  against the release's `SHA256SUMS.txt`: on Windows and Linux it replaces
  the app and restarts it, on macOS it opens the new disk image to drag to
  Applications. **Skip** keeps the current version.
  The app also checks by itself when it starts, at most once a week; when
  there is a newer version, the button becomes **Update to** and the
  version, and nothing else happens until you click it. Turn this off with
  **Check for updates once a week** in Settings. The app connects only to
  GitHub, only for these checks and updates.
- While converting, the Windows taskbar button shows the progress, and the
  button flashes (the Dock icon bounces on macOS) when a batch finishes
  while you're in another window.
- The window is drawn on the CPU and only redraws when something changes,
  so it uses almost no CPU or GPU while idle and doesn't flicker on variable
  refresh rate monitors (G-Sync, FreeSync).

</details>

## Command line

```
hdrshot [OPTIONS] <INPUTS>...
hdrshot [OPTIONS] --watch <FOLDER> [INPUTS]...
```

Inputs are image files or folders. Wildcards such as `*.jxr` work in cmd
and PowerShell too. Folders are searched for `.jxr`, `.exr`, `.hdr` and
`.jxl` files; HDR PNGs have to be given by name, since a folder of
converted images also holds the PNGs made from them.

### Examples

```sh
hdrshot shot.jxr                         # writes shot_hdr.png and shot_sdr.png
hdrshot --hdr -o out/ a.jxr b.jxr        # HDR only, into out/
hdrshot *.jxr                            # wildcards work in cmd and PowerShell too
hdrshot -r Captures/                     # a folder, including its subfolders
hdrshot --sdr --sdr-white 203 shot.jxr   # darker SDR result
hdrshot --format avif shot.jxr           # HDR as AVIF instead of PNG
hdrshot --sdr --sdr-format jpeg --jpeg-quality 85 shot.jxr
hdrshot --max-height 1440 --subfolders -o out/ Captures/
hdrshot --name "{name}-{type}" shot.jxr  # writes shot-hdr.png and shot-sdr.png
hdrshot --hdr --copy-small shot.jxr      # convert, then copy for Discord
hdrshot --watch Captures/ --copy         # convert each new screenshot, copy it
hdrshot --json Captures/ > results.jsonl # one JSON line per input, for scripts
```

### Behavior

Several files are converted at once, up to eight at a time. Each line of
output names a written or skipped file, a summary follows, and the exit code
is non-zero if any file failed.

Files that already exist are skipped, so converting a folder again only
adds the new screenshots. Each output gets the input's modified date, so
folders sorted by date stay in capture order. A failed conversion never
leaves a half-written file behind, and neither does Ctrl+C. If two inputs
would make the same file, for example `a.jxr` and `a.exr`, or two `shot.jxr`
from different folders with `-o`, the second is not converted and the error
says why. An SDR input only gets an SDR output.

Everything is automatic; the options below are only for tuning. Brightness
values are in nits and must be above 0.

### Inputs

| Option | Effect |
| --- | --- |
| `<INPUTS>...` | Image files or folders to convert. Required unless `--watch` is given |
| `-r, --recursive` | Also convert images in subfolders of the given folders |
| `--input-white <NITS>` | Brightness of 1.0 in OpenEXR, Radiance and float JPEG XL files (default: the file's own value, or 80) |

### Outputs

| Option | Effect |
| --- | --- |
| `--hdr` | Write only the HDR image, in the `--format` chosen |
| `--sdr` | Write only the tone-mapped SDR image. Giving both `--hdr` and `--sdr`, or neither, writes both |
| `-o, --output <DIR>` | Save into this folder instead of next to each input |
| `--name <PATTERN>` | Output file name without extension: `{name}` is the input's name, `{type}` is `hdr` or `sdr` (default `{name}_{type}`). The pattern needs `{name}` and can't contain folders; when both images go to one folder, it also needs `{type}` |
| `--subfolders` | Put the images in `hdr` and `sdr` subfolders of the output folder |
| `--max-height <PIXELS>` | Scale images taller than this down to this height, keeping the aspect ratio and averaging in linear light (default: keep the source resolution) |
| `--overwrite` | Replace files that already exist; by default they are skipped |
| `--no-keep-date` | Give the files the current time instead of the input's modified date |

### HDR image

| Option | Effect |
| --- | --- |
| `--format <png\|avif\|ultrahdr>` | File format of the HDR output (default `png`): 16-bit lossless PNG, 10-bit AVIF at about a twentieth of the size, or an Ultra HDR JPEG with a gain map, which is SDR in every viewer and HDR in those that read gain maps |
| `--hdr-peak <NITS>` | Limit the brightest highlight to this level, rolling off anything brighter smoothly (default: keep the original brightness) |
| `--avif-quality <1-100>` | AVIF quality (default 90); higher is closer to the original and larger |
| `--avif-speed <0-10>` | AVIF encoder speed (default 8); 0 is slowest with the smallest file, 10 is fastest |
| `--avif-420` | Store AVIF color at half resolution (4:2:0): smaller, but colored text and thin colored edges get softer |
| `--for-discord` | Leave `cICP` and `mDCV` out of HDR PNGs so Discord shows them correctly. Windows Photos and most HDR viewers then show them in SDR |

### SDR image

| Option | Effect |
| --- | --- |
| `--sdr-format <png\|jpeg>` | File format of the SDR output (default `png`). JPEG is a fraction of the size, but transparency is lost |
| `--jpeg-quality <1-100>` | JPEG quality of the SDR output with `--sdr-format jpeg` (default 90) |
| `--sdr-white <NITS>` | HDR brightness that becomes SDR white (default 80); lower is brighter |
| `--sdr-peak <NITS>` | HDR brightness that maps to the SDR maximum (default: the image's 99.9th percentile) |

### Clipboard

| Option | Effect |
| --- | --- |
| `--copy` | Copy the last file's PNG (the HDR one if made) to the clipboard, to paste into Discord or a folder. The copy leaves out the `cICP` chunk; AVIF and Ultra HDR results are copied as an HDR PNG made for Discord. See [Sharing on Discord](#sharing-on-discord) |
| `--copy-small` | Like `--copy`, but if the PNG is over Discord's 10 MB limit, copy it at 1440p or lower so it fits. The saved files stay as they are |

### Watch folder and originals

| Option | Effect |
| --- | --- |
| `--watch <FOLDER>` | Watch this folder and its subfolders, such as the Game Bar captures folder or the NVIDIA app's (`Videos\NVIDIA`), and convert each new image saved there until Ctrl+C. Images already in it are left alone; any inputs given are converted first. With `--copy`, each new result is copied |
| `--trash-originals` | Move each input to the Recycle Bin (Trash on macOS and Linux) once all of its outputs are written. Inputs that failed, or whose outputs already existed, are kept |

### Messages and scripting

| Option | Effect |
| --- | --- |
| `-q, --quiet` | Print only errors. Cannot be combined with `--json` |
| `--json` | Print one JSON object per input file instead of text |
| `-h, --help` | Print the help |
| `-V, --version` | Print the version |
| `--generate <SHELL\|man>` | Print a shell completion script (`bash`, `zsh`, `fish`, `powershell` or `elvish`) or the man page (`man`) and exit. It must be the only argument besides its value |

With `--json`, each line is one object. A converted input looks like this,
where `sdr_input` is `true` for SDR inputs:

```json
{"input":"shot.jxr","written":["shot_hdr.png","shot_sdr.png"],"skipped":[],"sdr_input":false}
```

An input that failed or was not found has an `error` instead:

```json
{"input":"broken.jxr","error":"..."}
```

### Exit codes

| Code | Meaning |
| --- | --- |
| 0 | All files converted (or skipped because they already existed) |
| 1 | At least one file failed or was not found, or the copy or the move to the Recycle Bin failed |
| 2 | Invalid arguments, such as an unknown option, a value out of range or an unusable `--name` pattern |
| 130 | Stopped with Ctrl+C |

### Shell completions and man page

The macOS and Linux command-line archives include shell completions for
bash, zsh and fish in `completions/`, and a man page in `man/hdrshot.1`. On
Windows, create the PowerShell completion script with
`hdrshot --generate powershell > _hdrshot.ps1`, then add
`. path\to\_hdrshot.ps1` to your PowerShell profile to load it.

## Sharing on Discord

Discord does not show every HDR file the way an HDR viewer does, so the
**Copy for Discord** button and `--copy` do not always copy the saved file
unchanged. The saved files are never modified.

| Saved HDR format | What Discord does with it | What gets copied |
| --- | --- | --- |
| PNG | Its image service converts PNGs that carry a `cICP` chunk to SDR with swapped colors, so warm scenes turn yellow. Without the chunk, it shows the image in HDR. | The same PNG without the `cICP` and `mDCV` chunks. The ICC profile, which also marks the image as BT.2020 PQ, stays and is kept under 32 KB, since Discord drops larger profiles. |
| AVIF | Ignores the PQ transfer, so the image looks far too bright. | An HDR PNG made for Discord and kept in a temporary folder. The desktop app makes it while converting the last image of a batch, and for the others when Copy for Discord is clicked. |
| Ultra HDR JPEG | Drops the gain map and shows only the SDR base image. | The same HDR PNG made for Discord. |

The saved PNG keeps `cICP` because Windows Photos and most HDR image viewers
need it to show the image in HDR; an ICC profile alone makes them show SDR.
`mDCV` is left out of the copy because the PNG specification only allows it
together with `cICP`. To save PNGs that are already in the Discord form,
turn on **Discord-compatible PNG** in the app or use `--for-discord`.

HDR in Discord works in the desktop app and in Chromium-based browsers on an
HDR display. The mobile apps do not reliably show HDR images, at least in
the inline preview. Discord's upload limit for free accounts is 10 MB, so the
app offers a smaller copy when the PNG is larger.

## How it works

Windows HDR screenshots store linear scRGB: BT.709 primaries, where 1.0 is
80 nits, values above 1.0 are highlights and negative values are colors
outside BT.709. Decoding uses [`jxr`](https://crates.io/crates/jxr), a pure
Rust JPEG XR decoder, so no C toolchain is needed.

| | HDR PNG | SDR PNG |
| --- | --- | --- |
| Color | Rec.2020, PQ (absolute nits) | sRGB |
| Bit depth | 16-bit, rounded to 10 bits | 8-bit |
| Metadata | `cICP`, `mDCV`, `cLLI`, ICC v4.4 with `cicp` tag and SDR fallback | sRGB |
| Brightness | Exactly the screenshot's | 80-nit SDR white, highlights rolled off |

<details>
<summary>The details</summary>

- **HDR**: converted to BT.2020 and encoded with PQ, keeping the
  screenshot's brightness exactly. Samples are rounded to 10 bits, the
  precision HDR displays use, which halves the file size. It is tagged as
  BT.2020 PQ both by a PNG `cICP` chunk, which Windows Photos and HDR image
  viewers need, and by an ICC v4.4 profile with a matching `cicp` tag, which
  Chrome and Edge also read, plus `mDCV` and `cLLI` describing the content's
  peak and average brightness so HDR viewers can fit it to the display.
  MaxCLL is the 99.99th percentile of pixel brightness, as the PNG spec
  recommends, so a few stray bright pixels don't make viewers dim the whole
  picture. SDR-only viewers that do color management (such as the Windows
  Photo Viewer) use the profile's lookup table instead, which applies the
  same SDR tone mapping as the SDR output, so they show a normal picture
  instead of a washed-out one.
- **AVIF**: the same BT.2020 PQ pixels as the HDR PNG, converted to
  10-bit full-range BT.2020 YCbCr (4:4:4 by default) and encoded with
  [`rav1e`](https://crates.io/crates/rav1e) in pure Rust. The container
  carries the matching `nclx` color box and `clli`/`mdcv` light levels.
  A 3440×1440 capture becomes about 450 KB instead of 11 MB. Encoding takes
  a few seconds per image.
- **Ultra HDR JPEG**: the base image is the SDR PNG's picture as a
  quality-95 4:4:4 JPEG. The gain map is a full-resolution grayscale JPEG
  holding, for each pixel, log2 of HDR over SDR brightness (relative to SDR
  white). Because the SDR tone map scales a pixel's three channels
  together, one channel restores the HDR picture exactly, apart from
  rounding and colors outside sRGB. The file follows the Ultra HDR 1.1
  layout: an XMP container directory and an MPF index in the base image,
  and the gain map's parameters as both Adobe `hdrgm` XMP and ISO 21496-1
  metadata. A 3440×1440 capture becomes about 2 MB.
- **Discord**: see [Sharing on Discord](#sharing-on-discord).
- **SDR**: scRGB 1.0 (80 nits) becomes SDR white by default, matching how
  Windows shows these screenshots in SDR viewers. Highlights above that roll
  off smoothly up to the image's 99.9th percentile brightness. The curve is
  applied to each pixel's brightest channel and the other channels are
  scaled with it, which keeps hue and saturation.
- **Transparency**: Game Bar captures carry an alpha channel that is always
  opaque, so it is dropped. A JXR with real transparency gives PNGs with an
  alpha channel; colors are weighted by alpha when scaling, so transparent
  pixels don't darken the edges.
- **Other inputs** are converted to the same linear scRGB first, so every
  output works the same for them:
  - **OpenEXR**: the first RGB layer, in the primaries of its
    `chromaticities` attribute (BT.709 if none). Its premultiplied alpha is
    undone. EXR is scene-referred, so 1.0 is the file's `whiteLuminance`,
    or 80 nits like scRGB; `--input-white` sets it.
  - **Radiance `.hdr`**: RGBE, BT.709, with the same 80-nit default.
  - **JPEG XL**: decoded with [`jxl-oxide`](https://crates.io/crates/jxl-oxide)
    (pure Rust). PQ and HLG files keep their absolute brightness; float
    files without an HDR transfer function are scene-linear like OpenEXR
    (1.0 is their intensity target if set, else 80 nits); other files are
    SDR and, like 8-bit JXR files, give only the SDR PNG. Animations give
    their first frame.
  - **HDR PNG**: PQ or HLG in BT.2020, BT.709 or Display P3, marked by a
    `cICP` chunk or an ICC `cicp` tag (so hdrshot's own HDR PNGs and their
    Discord copies both work). HLG uses a 1000-nit display, as BT.2100
    specifies. Folder scans leave PNGs out, so converted folders aren't
    converted again; give HDR PNGs by name. SDR PNGs are refused.
- **SDR JXR files** (8 bits per channel) have no HDR to keep, so only the
  SDR PNG is made, a faithful copy of the picture. The app and the command
  line say so.

</details>

## Building from source

With [Rust](https://rustup.rs) 1.99 or newer installed:

```sh
cargo build --release
cargo run --release -p hdrshot-gui   # desktop app
cargo run --release -p hdrshot -- --help
```

| Crate | Purpose |
| --- | --- |
| `crates/hdrshot-core` | Decoding, color conversion and encoding (library) |
| `crates/hdrshot-cli` | `hdrshot` command-line tool |
| `crates/hdrshot-gui` | Desktop app with drag-and-drop (egui) |
| `crates/egui-software-backend` | CPU renderer for the desktop app |

Captures are too large to keep in the repository. Point the sample test at
a folder of them:

```sh
HDRSHOT_SAMPLES=/path/to/jxr/folder cargo test --release -p hdrshot-core --test samples
```

Releases are built by GitHub Actions: pushing a `v*` tag builds all three
platforms and publishes the release with notes from
[CHANGELOG.md](https://github.com/snaketech-tu/hdrshot/blob/main/CHANGELOG.md).

## License

[Apache-2.0](https://github.com/snaketech-tu/hdrshot/blob/main/LICENSE)

The desktop app uses the [Inter](https://rsms.me/inter/) typeface by
Rasmus Andersson, under the
[SIL Open Font License 1.1](https://github.com/snaketech-tu/hdrshot/blob/main/crates/hdrshot-gui/assets/fonts/Inter-LICENSE.txt).

Made by snake.tech. If hdrshot is useful to you, you can support it on
[Ko-fi](https://ko-fi.com/snake_tech).
