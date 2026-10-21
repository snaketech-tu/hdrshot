# egui_software_backend (vendored)

The CPU rasterizer from
[egui_software_backend](https://github.com/DGriffin91/egui_software_backend)
0.0.3 by DGriffin91, licensed MIT OR Apache-2.0 (see the license files in
this folder). The published crate supports egui 0.34; this copy is ported to
egui 0.37 for the hdrshot desktop app, and published as `hdrshot-egui-backend`:

- `ahash` is a direct dependency, since egui no longer re-exports it.
- Texture updates are applied per batch, since `TexturesDelta::set` now holds
  several deltas per texture.
- The winit runner and test helpers are removed; the app has its own window
  code in `crates/hdrshot-gui/src/window.rs`.

Replace it with the crates.io release once one supports the egui version in
use.
