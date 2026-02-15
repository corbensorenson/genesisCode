#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

# Ensure target exists (idempotent).
rustup target add wasm32-wasip1 >/dev/null

cargo build -p gc_wasi_cli --target wasm32-wasip1 --release

WASM_PATH="target/wasm32-wasip1/release/genesis_wasi.wasm"
echo "$WASM_PATH"

