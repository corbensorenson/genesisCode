#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

cargo build -p gc_wasm --target wasm32-unknown-unknown

OUT_DIR="target/wasm-bindgen/gc_wasm"
mkdir -p "$OUT_DIR"

wasm-bindgen \
  --target nodejs \
  --out-dir "$OUT_DIR" \
  --out-name gc_wasm \
  target/wasm32-unknown-unknown/debug/gc_wasm.wasm

# Ensure Node treats generated bindings as CommonJS even when the repo root
# is configured as ESM (`"type": "module"`).
cat >"$OUT_DIR/package.json" <<'JSON'
{
  "type": "commonjs"
}
JSON

echo "$OUT_DIR/gc_wasm.js"
