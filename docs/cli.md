# hdrshot command line

The `hdrshot` command-line tool converts the same images as the desktop
app, with the same defaults. Download it from the
[latest release](https://github.com/snaketech-tu/hdrshot/releases/latest)
(`hdrshot-cli.exe` on Windows), or install it with Scoop or Cargo as
described in the [README](../README.md#download).

## Usage

```
hdrshot [OPTIONS] <INPUTS>...
hdrshot [OPTIONS] --watch <FOLDER> [INPUTS]...
```

Inputs are image files or folders. Wildcards such as `*.jxr` work in cmd
and PowerShell too. Folders are searched for `.jxr`, `.exr`, `.hdr`,
`.jxl` and `.heic` files, and for JPEGs and AVIFs that carry a gain map
(phone HDR photos). HDR PNGs and other HDR AVIFs have to be given by name,
since a folder of converted images also holds the PNGs and AVIFs made from
them; for the same reason, folder scans skip the JPEGs hdrshot writes.

## Examples

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

## Behavior

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

## Inputs

| Option | Effect |
| --- | --- |
| `<INPUTS>...` | Image files or folders to convert. Required unless `--watch` is given |
| `-r, --recursive` | Also convert images in subfolders of the given folders |
| `--input-white <NITS>` | Brightness of 1.0 in OpenEXR, Radiance and float JPEG XL files (default: the file's own value, or 80), and of SDR white in phone photos (default: 203) |

## Outputs

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

## HDR image

| Option | Effect |
| --- | --- |
| `--format <png\|avif\|ultrahdr>` | File format of the HDR output (default `png`): 16-bit lossless PNG, 10-bit AVIF at about a twentieth of the size, or an Ultra HDR JPEG with a gain map, which is SDR in every viewer and HDR in those that read gain maps |
| `--hdr-peak <NITS>` | Limit the brightest highlight to this level, rolling off anything brighter smoothly (default: keep the original brightness) |
| `--avif-quality <1-100>` | AVIF quality (default 90); higher is closer to the original and larger |
| `--avif-speed <0-10>` | AVIF encoder speed (default 8); 0 is slowest with the smallest file, 10 is fastest |
| `--avif-420` | Store AVIF color at half resolution (4:2:0): smaller, but colored text and thin colored edges get softer |
| `--for-discord` | Leave `cICP` and `mDCV` out of HDR PNGs so Discord shows them correctly. Windows Photos and most HDR viewers then show them in SDR |

## SDR image

| Option | Effect |
| --- | --- |
| `--sdr-format <png\|jpeg>` | File format of the SDR output (default `png`). JPEG is a fraction of the size, but transparency is lost |
| `--jpeg-quality <1-100>` | JPEG quality of the SDR output with `--sdr-format jpeg` (default 90) |
| `--sdr-white <NITS>` | HDR brightness that becomes SDR white (default 80); lower is brighter |
| `--sdr-peak <NITS>` | HDR brightness that maps to the SDR maximum (default: the image's 99.9th percentile) |

## Clipboard

| Option | Effect |
| --- | --- |
| `--copy` | Copy the last file's PNG (the HDR one if made) to the clipboard, to paste into Discord or a folder. The copy leaves out the `cICP` chunk; AVIF and Ultra HDR results are copied as an HDR PNG made for Discord. See [Sharing on Discord](../README.md#sharing-on-discord) |
| `--copy-small` | Like `--copy`, but if the PNG is over Discord's 10 MB limit, copy it at 1440p or lower so it fits. The saved files stay as they are |

## Watch folder and originals

| Option | Effect |
| --- | --- |
| `--watch <FOLDER>` | Watch this folder and its subfolders, such as the Game Bar captures folder or the NVIDIA app's (`Videos\NVIDIA`), and convert each new image saved there until Ctrl+C. Images already in it are left alone; any inputs given are converted first. With `--copy`, each new result is copied |
| `--trash-originals` | Move each input to the Recycle Bin (Trash on macOS and Linux) once all of its outputs are written. Inputs that failed, or whose outputs already existed, are kept |

## Messages and scripting

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

## Exit codes

| Code | Meaning |
| --- | --- |
| 0 | All files converted (or skipped because they already existed) |
| 1 | At least one file failed or was not found, or the copy or the move to the Recycle Bin failed |
| 2 | Invalid arguments, such as an unknown option, a value out of range or an unusable `--name` pattern |
| 130 | Stopped with Ctrl+C |

## Shell completions and man page

The macOS and Linux command-line archives include shell completions for
bash, zsh and fish in `completions/`, and a man page in `man/hdrshot.1`. On
Windows, create the PowerShell completion script with
`hdrshot --generate powershell > _hdrshot.ps1`, then add
`. path\to\_hdrshot.ps1` to your PowerShell profile to load it.
