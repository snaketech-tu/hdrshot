# Contributing to hdrshot

Bug reports and ideas are welcome as
[issues](https://github.com/snaketech-tu/hdrshot/issues/new/choose), questions
in [Discussions](https://github.com/snaketech-tu/hdrshot/discussions). For a bug,
the app version, your operating system and where the image came from (Game
Bar, the NVIDIA app, a phone, another program) help the most. If you can,
attach the original image or a link to it.

Pull requests are welcome too. For a larger change, please open an issue
first so we can agree on the approach.

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
| `crates/hdrshot-web`, `web/` | Browser version (WebAssembly) |

To build and try the browser version, add the `wasm32-unknown-unknown` target
and install the [wasm-bindgen CLI](https://github.com/wasm-bindgen/wasm-bindgen)
of the version in `Cargo.lock`, then:

```sh
web/build.sh
python3 -m http.server -d target/web
```

Captures are too large to keep in the repository. Point the sample test at
a folder of them:

```sh
HDRSHOT_SAMPLES=/path/to/jxr/folder cargo test --release -p hdrshot-core --test samples
```

Releases are built by GitHub Actions: pushing a `v*` tag, or running the
Release workflow with a tag name, builds all three platforms and publishes
the release with notes from
[CHANGELOG.md](https://github.com/snaketech-tu/hdrshot/blob/main/CHANGELOG.md).
After each release, the browser version is published to GitHub Pages from
the same commit.

## Before opening a pull request

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

