#!/bin/sh
set -eu
app_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
project_dir=$(CDPATH= cd -- "$app_dir/.." && pwd)
cd "$project_dir"
mkdir -p dist
bridge_tmp=$(mktemp "$project_dir/dist/.quiet-bridge.XXXXXX.mjs")
binary_tmp=$(mktemp "$app_dir/.tikho.XXXXXX")
trap 'rm -f "$bridge_tmp" "$binary_tmp"' EXIT HUP INT TERM
./node_modules/.bin/esbuild source/quiet/bridge.ts --bundle --platform=node --format=esm --packages=external --outfile="$bridge_tmp"
mv -f "$bridge_tmp" dist/quiet-bridge.js
if [ "${CALMIG_CARGO_OFFLINE:-0}" = 1 ]; then
  cargo build --manifest-path quiet-desktop/src-tauri/Cargo.toml --offline
else
  cargo build --manifest-path quiet-desktop/src-tauri/Cargo.toml
fi
build_dir=${CARGO_TARGET_DIR:-"$app_dir/src-tauri/target"}
cp "$build_dir/debug/tikho-desktop" "$binary_tmp"
if command -v strip >/dev/null 2>&1; then strip --strip-debug "$binary_tmp"; fi
chmod 755 "$binary_tmp"
mv -f "$binary_tmp" "$app_dir/tikho"
