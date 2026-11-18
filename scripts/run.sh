#!/usr/bin/env bash
set -euo pipefail
desklink_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$desklink_root"
if command -v cargo >/dev/null 2>&1; then
  exec cargo run --locked --release "$@"
elif [ -x "$desklink_root/.tools/cargo/bin/cargo" ]; then
  export CARGO_HOME="$desklink_root/.tools/cargo"
  export RUSTUP_HOME="$desklink_root/.tools/rustup"
  exec "$CARGO_HOME/bin/cargo" run --locked --release "$@"
else
  echo 'Rust is required. Install it from https://rustup.rs, then run this script again.' >&2
  exit 1
fi
