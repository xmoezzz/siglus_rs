#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PLATFORM_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
ROOT_DIR="$(cd "$PLATFORM_DIR/.." && pwd)"

# The app library package. Deliberately not SIGLUS_CARGO_PKG: CI sets that to
# siglus_scene_vm for the desktop binary builds.
PKG="${LAUNCHER_CARGO_PKG:-game_launcher}"
WASM_BINDGEN_TARGET="${WASM_BINDGEN_TARGET:-web}"
# The CLI and Rust crate share a versioned, unstable binary schema.
# Read Cargo's locked version instead of maintaining a second independent pin.
WASM_BINDGEN_VERSION="$(awk '
  /^\[\[package\]\]$/ { bindgen = 0 }
  /^name = "wasm-bindgen"$/ { bindgen = 1 }
  bindgen && /^version = / { gsub(/"/, "", $3); print $3; exit }
' "$ROOT_DIR/Cargo.lock")"
if [[ -z "$WASM_BINDGEN_VERSION" ]]; then
  echo "ERROR: wasm-bindgen is missing from Cargo.lock" >&2
  exit 1
fi
DIST="$ROOT_DIR/dist/wasm"
PKG_DIR="$DIST/pkg"

mkdir -p "$DIST" "$PKG_DIR"

command -v cargo >/dev/null 2>&1 || { echo "ERROR: cargo not found" >&2; exit 1; }
command -v rustup >/dev/null 2>&1 || { echo "ERROR: rustup not found" >&2; exit 1; }

ensure_wasm_bindgen() {
  if ! command -v wasm-bindgen >/dev/null 2>&1; then
    echo "[wasm] wasm-bindgen not found; installing wasm-bindgen-cli ${WASM_BINDGEN_VERSION}"
    cargo install -f wasm-bindgen-cli --version "${WASM_BINDGEN_VERSION}" --locked
    return 0
  fi

  local current_version
  current_version="$(wasm-bindgen --version | awk '{print $2}')"
  if [[ "${current_version}" != "${WASM_BINDGEN_VERSION}" ]]; then
    echo "[wasm] wasm-bindgen version mismatch: current=${current_version}, required=${WASM_BINDGEN_VERSION}"
    echo "[wasm] reinstalling wasm-bindgen-cli ${WASM_BINDGEN_VERSION}"
    cargo install -f wasm-bindgen-cli --version "${WASM_BINDGEN_VERSION}" --locked
  else
    echo "[wasm] wasm-bindgen version OK: ${current_version}"
  fi
}

ensure_wasm_bindgen
rustup target add wasm32-unknown-unknown

cargo build --release --locked -p "$PKG" --target wasm32-unknown-unknown

WASM_PATH="$ROOT_DIR/target/wasm32-unknown-unknown/release/${PKG}.wasm"
test -f "$WASM_PATH"

rm -rf "$PKG_DIR"
mkdir -p "$PKG_DIR"

wasm-bindgen "$WASM_PATH" \
  --target "$WASM_BINDGEN_TARGET" \
  --out-dir "$PKG_DIR"

cp -f "$PLATFORM_DIR/wasm/index.html" "$DIST/index.html"
cp -f "$PLATFORM_DIR/wasm/main.js" "$DIST/main.js"
cp -f "$PLATFORM_DIR/wasm/style.css" "$DIST/style.css"

(
  cd "$DIST"
  rm -f siglus_rs-wasm.zip
  zip -r siglus_rs-wasm.zip index.html main.js style.css pkg
)

echo "OK: $DIST/siglus_rs-wasm.zip"
