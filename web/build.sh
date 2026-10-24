#!/bin/sh
# Builds the browser version into target/web: the page, the WebAssembly
# build of hdrshot-core (crates/hdrshot-web) and the font and icons it uses.
# Needs the wasm32-unknown-unknown Rust target and the wasm-bindgen CLI of
# the version crates/hdrshot-web pins. wasm-opt (binaryen) is used when
# installed and makes the module about a quarter smaller.
#
# To try it: web/build.sh && python3 -m http.server -d target/web
set -eu
cd "$(dirname "$0")/.."
out=target/web
wasm=target/wasm32-unknown-unknown/release/hdrshot_web.wasm

cargo build --release --locked -p hdrshot-web --target wasm32-unknown-unknown
rm -rf "$out"
mkdir -p "$out/pkg" "$out/fonts"
wasm-bindgen --target web --no-typescript --out-dir "$out/pkg" "$wasm"
if command -v wasm-opt > /dev/null; then
  wasm-opt -O3 --enable-bulk-memory --enable-nontrapping-float-to-int \
    --enable-sign-ext --enable-mutable-globals --enable-reference-types \
    --enable-multivalue -o "$out/pkg/hdrshot_web_bg.wasm" "$out/pkg/hdrshot_web_bg.wasm"
fi
cp web/index.html web/style.css web/app.js web/worker.js web/zip.js "$out/"
cp crates/hdrshot-gui/assets/fonts/Inter-Regular.ttf \
  crates/hdrshot-gui/assets/fonts/Inter-SemiBold.ttf \
  crates/hdrshot-gui/assets/fonts/Inter-LICENSE.txt "$out/fonts/"
cp packaging/linux/icons/hicolor/64x64/apps/hdrshot.png "$out/icon-64.png"
cp packaging/linux/icons/hicolor/256x256/apps/hdrshot.png "$out/icon-256.png"
ls -l "$out/pkg"
