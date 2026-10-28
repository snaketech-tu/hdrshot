# hdrshot-core

The library behind [hdrshot](https://github.com/snaketech-tu/hdrshot), an HDR
image tool. It reads HDR images and writes them as HDR PNG, AVIF, Ultra HDR
JPEG or tone-mapped SDR PNG. Everything is pure Rust.

Inputs: JPEG XR (`.jxr`, `.wdp`), OpenEXR (`.exr`), Radiance (`.hdr`), HDR PNG
(PQ or HLG, signalled with cICP or an ICC profile), JPEG XL (`.jxl`), HDR AVIF
(PQ, HLG or an ISO 21496-1 gain map, decoded with rav1d; not on WebAssembly),
and phone HDR photos: JPEGs with a gain map (Ultra HDR, ISO 21496-1 or Apple's)
and HEIC (iPhone gain maps, PQ and HLG).

Outputs:

- HDR PNG: 16-bit BT.2020 PQ with cICP, mDCV, cLLI and an ICC profile.
- AVIF: 10-bit BT.2020 PQ, encoded with rav1e.
- Ultra HDR JPEG: an SDR JPEG with an ISO 21496-1 gain map.
- SDR PNG: 8-bit sRGB, tone-mapped from the HDR image.

```rust
use hdrshot_core::{ConvertOptions, convert_file};

fn main() -> Result<(), hdrshot_core::Error> {
    let outputs = convert_file("capture.jxr".as_ref(), &ConvertOptions::default())?;
    println!("{outputs:?}");
    Ok(())
}
```

`rav1d` can panic on damaged AVIF files inside its C interface, which would
abort the program, so hdrshot-core decodes each AVIF on a thread of its own
and turns such a panic into an error. It installs a panic hook for this the
first time it reads an AVIF; after a damaged one, don't replace the panic hook
(`std::panic::set_hook` or `take_hook`), as that would wait forever.

Licensed under Apache-2.0.
