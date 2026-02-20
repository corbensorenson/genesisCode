#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

echo "panic-guard: checking production crates for unwrap/expect/panic usage"
cargo clippy \
  -p gc_cli_driver \
  -p gc_wasm \
  -p gc_obligations \
  -p gc_effects \
  -p gc_registry \
  -p gc_kernel \
  --lib \
  -- \
  -D clippy::unwrap_used \
  -D clippy::expect_used \
  -D clippy::panic

echo "panic-guard: checking production binaries for unwrap/expect/panic usage"
cargo clippy \
  -p gc_cli \
  --bin genesis \
  -- \
  -D clippy::unwrap_used \
  -D clippy::expect_used \
  -D clippy::panic

cargo clippy \
  -p gc_wasi_cli \
  --bin genesis_wasi \
  -- \
  -D clippy::unwrap_used \
  -D clippy::expect_used \
  -D clippy::panic

echo "panic-guard: ok"
