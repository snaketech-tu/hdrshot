<div align="center">

<img src="https://raw.githubusercontent.com/snaketech-tu/hdrshot/main/crates/hdrshot-gui/assets/icon.png" width="128" alt="hdrshot icon">

# hdrshot

**HDR image tool. Converts HDR screenshots and images to HDR PNG, AVIF,
Ultra HDR JPEG and SDR PNG.**

[![Release](https://img.shields.io/github/v/release/snaketech-tu/hdrshot?style=flat-square&color=0e7fc0)](https://github.com/snaketech-tu/hdrshot/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/snaketech-tu/hdrshot/total?style=flat-square&color=0e7fc0)](https://github.com/snaketech-tu/hdrshot/releases)
[![CI](https://img.shields.io/github/actions/workflow/status/snaketech-tu/hdrshot/ci.yml?branch=main&style=flat-square&label=CI)](https://github.com/snaketech-tu/hdrshot/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/hdrshot?style=flat-square&color=0e7fc0)](https://crates.io/crates/hdrshot)
[![License](https://img.shields.io/github/license/snaketech-tu/hdrshot?style=flat-square)](https://github.com/snaketech-tu/hdrshot/blob/main/LICENSE)
![Platforms](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-555?style=flat-square)

[**Download**](https://github.com/snaketech-tu/hdrshot/releases/latest) ·
[Desktop app](#desktop-app) ·
[Command line](#command-line) ·
[How it works](#how-it-works)

<img src="https://raw.githubusercontent.com/snaketech-tu/hdrshot/main/docs/images/app.png" width="560" alt="The hdrshot desktop app after converting a batch of screenshots">

</div>

Windows Game Bar and Xbox save HDR screenshots as JPEG XR, a format few
applications can open. hdrshot converts them, and other HDR images, to
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
- **SDR PNG.** Tone-mapped the way Windows shows the
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
- **Double-click to convert.** Optionally opens `.jxr` files with hdrshot
  on Windows and Linux.
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

`SHA256SUMS.txt` in each release lists the checksums of all downloads.

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

With [Rust](https://rustup.rs) installed, both tools can also be built from
[crates.io](https://crates.io/crates/hdrshot):

```sh
cargo install hdrshot        # command-line tool
cargo install hdrshot-gui    # desktop app
```

## Desktop app

1. Drop `.jxr` files or folders onto the window, click the drop area to
   choose files, or paste files copied in Explorer or Finder with Ctrl+V.
   On Windows and Linux, files dropped onto the program's icon are added
   too. Files opened while the app is running, for example several
   screenshots picked with **Open with**, go to the window that is already
   open.
2. Choose **HDR + SDR**, **HDR only** or **SDR only**, the HDR format
   (**PNG**, **AVIF** or **Ultra HDR JPEG**), and whether to save next to
   each image or into one folder.
3. Press **Convert**.

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
  saved PNGs stay at full size. With AVIF or Ultra HDR JPEG as the HDR
  format, the copy is an HDR PNG made for Discord, since Discord shows
  AVIF too bright and Ultra HDR JPEGs in SDR only. The app makes it while
  converting and keeps it in a temporary folder, so copying is instant.
- **Previews.** Each file gets a thumbnail; hover it for a larger SDR
  preview. When the Advanced settings are changed, the preview shows the
  automatic result and the result with your settings, so you can compare.
- **The file list.** Double-click a converted image to open it.
  Right-click a row to open the HDR or SDR PNG, show it in its folder,
  convert it again or remove it. **Clear** removes all files or only the
  converted ones, and **Retry failed** converts the failed ones again.
  Click a row to select it; Delete removes it, Enter converts the list and
  the arrow keys move the selection.
- **PNGs that already exist.** When some of the PNGs a batch would make
  are already there, the app asks once per batch whether to replace them,
  keep both (the new ones get a number, e.g. `shot_hdr (2).png`) or skip
  them.
- **Resolution** scales the output down to 2160p, 1440p, 1080p or 720p,
  keeping the aspect ratio and never scaling up. It starts at Source every
  time.
- **Open .jxr files with hdrshot** (Windows and Linux) makes a double-click
  on a screenshot open it in the app. Windows asks you to confirm the choice
  in Settings.
  On macOS, hdrshot is listed under **Open With** for `.jxr` files; to make
  it the default, choose it in the file's **Get Info** window and click
  **Change All**.
- **Advanced** has **Keep the capture date** (on by default), which gives
  each PNG the date of its screenshot so folders sorted by date stay in
  capture order. **Save HDR and SDR PNGs in subfolders** writes them
  to `hdr` and `sdr` subfolders, and with that on, **End file names in _hdr
  and _sdr** can be turned off. It also holds the tuning options from the
  command line.
  They are off by default and reset every time the app starts, so results
  always match the original screenshot unless you change them.
- **Check for updates**, at the bottom right, looks for a newer release on
  GitHub and shows its release notes. **Update** downloads it: on Windows
  and Linux it replaces the app and restarts it, on macOS it opens the new
  disk image to drag to Applications. **Skip** keeps the current version.
  The app only connects to GitHub when you press the button.
- While converting, the Windows taskbar button shows the progress, and the
  button flashes (the Dock icon bounces on macOS) when a batch finishes
  while you're in another window.
- The window is drawn on the CPU and only redraws when something changes,
  so it uses almost no CPU or GPU while idle and doesn't flicker on variable
  refresh rate monitors (G-Sync, FreeSync).

</details>

## Command line

```sh
hdrshot shot.jxr                         # writes shot_hdr.png and shot_sdr.png
hdrshot --hdr -o out/ a.jxr b.jxr        # HDR only, into out/
hdrshot *.jxr                            # wildcards work in cmd and PowerShell too
hdrshot -r Captures/                     # a folder, including its subfolders
hdrshot --sdr --sdr-white 203 shot.jxr   # darker SDR result
```

Several files are converted at once, up to eight at a time. Each line of
output names a written or skipped PNG, a summary follows, and the exit code
is non-zero if any file failed.

PNGs that already exist are skipped, so converting a folder again only
adds the new screenshots. Each PNG gets the screenshot's modified date, so
folders sorted by date stay in capture order. A failed conversion never
leaves a half-written PNG behind.

Everything is automatic; these options are only for tuning:

| Option | Effect |
| --- | --- |
| `--hdr` / `--sdr` | Make only the HDR or only the SDR PNG |
| `-r, --recursive` | Also convert files in subfolders of the given folders |
| `-o, --output <DIR>` | Save into this folder instead of next to each input |
| `--format <png\|avif\|ultrahdr>` | File format of the HDR output (default `png`) |
| `--avif-quality <1-100>` | AVIF quality (default 90); lower is smaller |
| `--avif-speed <0-10>` | AVIF encoder speed (default 8); 10 is faster but larger |
| `--avif-420` | Store AVIF color at half resolution: smaller, softer colored edges |
| `--hdr-peak <NITS>` | Limit the HDR PNG's brightest highlight, rolling off anything brighter |
| `--sdr-white <NITS>` | HDR brightness that becomes SDR white (default 80; lower is brighter) |
| `--sdr-peak <NITS>` | Brightness that maps to full SDR white, instead of the 99.9th percentile |
| `--input-white <NITS>` | Brightness of 1.0 in OpenEXR, Radiance and float JPEG XL files (default: the file's own value, or 80) |
| `--name <PATTERN>` | Output file name without extension: `{name}` is the input's name, `{type}` is `hdr` or `sdr` (default `{name}_{type}`) |
| `--subfolders` | Put the PNGs in `hdr` and `sdr` subfolders of the output folder |
| `--max-height <PIXELS>` | Scale taller images down to this height, averaging in linear light |
| `--overwrite` | Replace PNGs that already exist; by default they are skipped |
| `--no-keep-date` | Give the PNGs the current time instead of the screenshot's date |
| `--copy` | Copy the last file's PNG (the HDR one if made) to the clipboard, ready for Discord |
| `--copy-small` | Like `--copy`, but scale the copy down if it is over Discord's 10 MB limit |
| `-q, --quiet` | Print only errors |
| `--json` | Print one JSON object per input file, for scripts |

Run `hdrshot --help` for the full list.

The macOS and Linux command-line archives include shell completions for
bash, zsh and fish in `completions/`, and a man page in `man/hdrshot.1`. On
Windows, create the PowerShell completion script with
`hdrshot --generate powershell > _hdrshot.ps1`, then add
`. path\to\_hdrshot.ps1` to your PowerShell profile to load it.

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
- **Discord**: its image service converts PNGs with a `cICP` chunk to SDR
  with wrong colors (warm scenes turn yellow) but shows them correctly
  without it, so the clipboard copy leaves the chunk out (and `mDCV`,
  which the PNG spec only allows together with `cICP`). It ignores the
  PQ transfer of AVIF files, so they look too bright, and drops the gain
  map of Ultra HDR JPEGs, so they show in SDR; copies of those outputs are
  therefore made as HDR PNGs.
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

With [Rust](https://rustup.rs) installed:

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

Made by snake.tech. If hdrshot is useful to you, you can support it on
[Ko-fi](https://ko-fi.com/snake_tech).
