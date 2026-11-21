#!/usr/bin/env bash
set -euo pipefail
desklink_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$desklink_root"
if [ "$(uname -s)" != Darwin ]; then
  echo 'Run this script on macOS.' >&2
  exit 1
fi
if ! command -v cargo >/dev/null 2>&1 && [ -x "$desklink_root/.tools/cargo/bin/cargo" ]; then
  export CARGO_HOME="$desklink_root/.tools/cargo"
  export RUSTUP_HOME="$desklink_root/.tools/rustup"
  export PATH="$CARGO_HOME/bin:$PATH"
fi
cargo build --locked --release --target aarch64-apple-darwin --target-dir "$desklink_root/target" --bin desklink
desklink_bundle="$desklink_root/dist/DeskLink.app"
mkdir -p "$desklink_bundle/Contents/MacOS"
mkdir -p "$desklink_bundle/Contents/Resources"
lipo -verify_arch arm64 "$desklink_root/target/aarch64-apple-darwin/release/desklink"
cp "$desklink_root/target/aarch64-apple-darwin/release/desklink" "$desklink_bundle/Contents/MacOS/DeskLink"
cp packaging/macos/Info.plist "$desklink_bundle/Contents/Info.plist"
cp assets/icons/DeskLink.icns "$desklink_bundle/Contents/Resources/DeskLink.icns"
codesign --force --deep --sign "${DESKLINK_SIGNING_IDENTITY:--}" "$desklink_bundle"
echo "Built $desklink_bundle"
