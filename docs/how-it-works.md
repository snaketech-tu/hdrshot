# How hdrshot works

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

## Details

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
- **Discord**: see [Sharing on Discord](../README.md#sharing-on-discord).
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
  - **HDR AVIF** (`.avif`): PQ or HLG AVIFs, such as hdrshot's own and
    HDR exports from Photoshop and Lightroom, keep their absolute
    brightness like HDR PNGs. AVIFs with an ISO 21496-1 gain map (marked by
    the `tmap` brand) whose main picture is SDR are read like phone photos,
    below. Grids, alpha, rotation and mirroring are supported. Decoded with
    [`rav1d`](https://crates.io/crates/rav1d), the Rust port of dav1d,
    built without its assembly. Folder scans take only AVIFs with a gain
    map, since hdrshot's own AVIFs have none; give other AVIFs by name.
    SDR AVIFs are refused. Not available in the browser version, where
    `rav1d` doesn't build.
  - **Phone HDR photos** are an SDR picture plus a gain map that says how
    much brighter each pixel is in HDR. hdrshot applies the whole gain map,
    giving the picture a phone shows on a display with enough headroom.
    SDR white is 203 nits, the reference white of ITU-R BT.2408, and the
    SDR output keeps it unless `--sdr-white` is changed, so it looks like
    the photo's own SDR version. `--input-white` sets another white.
    - **JPEG** (`.jpg`): Ultra HDR, with Adobe `hdrgm` XMP or ISO 21496-1
      metadata, and iPhone JPEGs with Apple's gain map, whose headroom is
      read from XMP or the EXIF maker note by Apple's published formula.
      Decoded with [`zune-jpeg`](https://crates.io/crates/zune-jpeg). The
      EXIF orientation is applied. JPEGs without a gain map are refused.
    - **HEIC** (`.heic`, `.heif`, `.hif`): iPhone photos, with Apple's gain
      map or the ISO 21496-1 one of iOS 18 and later, decoded with
      [`heic-decoder`](https://crates.io/crates/heic-decoder) (pure Rust).
      PQ and HLG HEIFs, as some cameras save them, keep their absolute
      brightness like HDR PNGs.
- **SDR JXR files** (8 bits per channel) have no HDR to keep, so only the
  SDR PNG is made, a faithful copy of the picture. The app and the command
  line say so.

