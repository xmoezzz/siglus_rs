#!/usr/bin/env bash
set -euo pipefail


SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PLATFORM_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
ROOT_DIR="$(cd "$PLATFORM_DIR/.." && pwd)"

IOS_DIR="$PLATFORM_DIR/ios/SiglusLauncher"
HDR_DIR="$IOS_DIR/Headers"
VENDOR_DIR="$IOS_DIR/Vendor"
OUT_XCF="$VENDOR_DIR/Siglus.xcframework"

SIGLUS_CARGO_PKG="${SIGLUS_CARGO_PKG:-siglus_scene_vm}"
RUST_LIB_NAME="${RUST_LIB_NAME:-siglus_scene_vm}" # cargo output: lib${RUST_LIB_NAME}.a
LIB_NAME="${LIB_NAME:-siglus}" # xcframework public library name: lib${LIB_NAME}.a

TGT_IOS="aarch64-apple-ios"
TGT_SIM="aarch64-apple-ios-sim"

command -v cargo >/dev/null 2>&1 || { echo "ERROR: cargo not found" >&2; exit 1; }
command -v rustup >/dev/null 2>&1 || { echo "ERROR: rustup not found" >&2; exit 1; }
command -v xcodebuild >/dev/null 2>&1 || { echo "ERROR: xcodebuild not found (install Xcode)" >&2; exit 1; }
command -v xcrun >/dev/null 2>&1 || { echo "ERROR: xcrun not found (install Xcode)" >&2; exit 1; }

[[ -d "$IOS_DIR" ]] || { echo "ERROR: Missing iOS launcher dir: $IOS_DIR" >&2; exit 1; }
[[ -d "$HDR_DIR" ]] || { echo "ERROR: Missing headers dir: $HDR_DIR" >&2; exit 1; }
[[ -f "$HDR_DIR/siglus.h" ]] || { echo "ERROR: Missing header: $HDR_DIR/siglus.h" >&2; exit 1; }

mkdir -p "$VENDOR_DIR"

# Ensure Rust targets
rustup target add "$TGT_IOS" >/dev/null 2>&1 || true
rustup target add "$TGT_SIM" >/dev/null 2>&1 || true

echo "[ios-xcf] Building Rust static libs..."
pushd "$ROOT_DIR" >/dev/null
cargo build --release -p "$SIGLUS_CARGO_PKG" --target "$TGT_IOS"
cargo build --release -p "$SIGLUS_CARGO_PKG" --target "$TGT_SIM"
popd >/dev/null

LIB_IOS_RUST_A="$ROOT_DIR/target/$TGT_IOS/release/lib${RUST_LIB_NAME}.a"
LIB_SIM_RUST_A="$ROOT_DIR/target/$TGT_SIM/release/lib${RUST_LIB_NAME}.a"
TMP_LIB_DIR="$VENDOR_DIR/.siglus-xcframework-input"
LIB_IOS_A="$TMP_LIB_DIR/ios-arm64/lib${LIB_NAME}.a"
LIB_SIM_A="$TMP_LIB_DIR/ios-arm64-simulator/lib${LIB_NAME}.a"

if [[ ! -f "$LIB_IOS_RUST_A" ]]; then
  echo "ERROR: Missing iOS static lib: $LIB_IOS_RUST_A" >&2
  echo "Hint: ensure siglus_scene_vm outputs staticlib for iOS." >&2
  exit 1
fi
if [[ ! -f "$LIB_SIM_RUST_A" ]]; then
  echo "ERROR: Missing iOS simulator static lib: $LIB_SIM_RUST_A" >&2
  exit 1
fi

rm -rf "$OUT_XCF" "$TMP_LIB_DIR"
mkdir -p "$(dirname "$LIB_IOS_A")" "$(dirname "$LIB_SIM_A")"
cp -f "$LIB_IOS_RUST_A" "$LIB_IOS_A"
cp -f "$LIB_SIM_RUST_A" "$LIB_SIM_A"

echo "[ios-xcf] Creating xcframework..."
xcodebuild -create-xcframework \
  -library "$LIB_IOS_A" -headers "$HDR_DIR" \
  -library "$LIB_SIM_A" -headers "$HDR_DIR" \
  -output "$OUT_XCF"

rm -rf "$TMP_LIB_DIR"
echo "[ios-xcf] OK: $OUT_XCF"


