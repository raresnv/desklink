#!/usr/bin/env bash
set -euo pipefail
desklink_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$desklink_root"
printenv DESKLINK_LLVM_MINGW >/dev/null || { echo 'Set DESKLINK_LLVM_MINGW to a verified LLVM-MinGW installation.' >&2; exit 1; }
desklink_bin="$DESKLINK_LLVM_MINGW/bin"
[ -x "$desklink_bin/aarch64-w64-mingw32-clang" ] || { echo 'ARM64 compiler is missing.' >&2; exit 1; }
if ! command -v cargo >/dev/null 2>&1 && [ -x .tools/cargo/bin/cargo ]; then
  export CARGO_HOME="$desklink_root/.tools/cargo" RUSTUP_HOME="$desklink_root/.tools/rustup"
  export PATH="$CARGO_HOME/bin:$PATH"
fi
CARGO_TARGET_AARCH64_PC_WINDOWS_GNULLVM_LINKER="$desklink_bin/aarch64-w64-mingw32-clang" \
CC_aarch64_pc_windows_gnullvm="$desklink_bin/aarch64-w64-mingw32-clang" \
AR_aarch64_pc_windows_gnullvm="$desklink_bin/aarch64-w64-mingw32-ar" \
DESKLINK_WINDRES="$desklink_bin/aarch64-w64-mingw32-windres" \
cargo build --locked --release --target aarch64-pc-windows-gnullvm --bin desklink
