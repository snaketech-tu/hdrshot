# jxr2png

Converts HDR JPEG XR (`.jxr`) images, such as Windows Game Bar and Xbox HDR
screenshots, to PNG. Runs on Windows, macOS and Linux.

Each input can produce:

- **HDR PNG**: 16-bit, Rec.2020 primaries with the PQ transfer function and a
  `cICP` chunk, so HDR-aware viewers and browsers display it in HDR.
- **SDR PNG**: tone-mapped 8-bit sRGB for everything else.

> Work in progress: conversion works from the command line. The desktop
> app is coming next.

## How the conversion works

Windows HDR screenshots store linear scRGB: BT.709 primaries, where 1.0 is
80 nits, values above 1.0 are highlights and negative values are colors
outside BT.709. Decoding uses [`jxr`](https://crates.io/crates/jxr), a pure
Rust JPEG XR decoder, so no C toolchain is needed.

- **HDR**: converted to BT.2020, encoded with PQ (absolute nits) and tagged
  with `cICP` (BT.2020, PQ, full range) and `cLLI` (MaxCLL/MaxFALL).
- **SDR**: the HDR reference white (203 nits by default, from ITU-R BT.2408)
  becomes SDR white. Highlights above that roll off smoothly up to the
  image's 99.9th percentile brightness. The curve is applied to each pixel's
  brightest channel and the other channels are scaled with it, which keeps
  hue and saturation.

## Layout

| Crate | Purpose |
| --- | --- |
| `crates/jxr2png-core` | Decoding, color conversion and PNG encoding |
| `crates/jxr2png-cli` | `jxr2png` command-line tool |
| `crates/jxr2png-gui` | Desktop app with drag-and-drop (egui) |

## Building

```sh
cargo build --release
```

## Command line

```sh
jxr2png shot.jxr                 # writes shot_hdr.png and shot_sdr.png
jxr2png --sdr -o out/ *.jxr      # SDR only, into out/
jxr2png --sdr --sdr-white 160 shot.jxr   # brighter SDR result
```

`--sdr-white <NITS>` sets which HDR brightness becomes SDR white (lower is
brighter). `--sdr-peak <NITS>` sets the brightness that maps to full SDR
white instead of using the 99.9th percentile.

## Testing with real screenshots

Captures are too large to keep in the repository. Point the sample test at
a folder of them:

```sh
JXR2PNG_SAMPLES=/path/to/jxr/folder cargo test --release -p jxr2png-core --test samples
```

## License

Apache-2.0
