#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "wasm-bindgen-web" \
  ".genesis/build/cargo" \
  "GENESIS_WASM_BINDGEN_WEB_CARGO_TARGET_DIR"

cargo build -p gc_wasm --target wasm32-unknown-unknown

OUT_DIR="$CARGO_TARGET_DIR/wasm-bindgen-web/gc_wasm"
mkdir -p "$OUT_DIR"

wasm-bindgen \
  --target web \
  --out-dir "$OUT_DIR" \
  --out-name gc_wasm \
  "$CARGO_TARGET_DIR/wasm32-unknown-unknown/debug/gc_wasm.wasm"

echo "$OUT_DIR/gc_wasm.js"
