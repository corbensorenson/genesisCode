#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "selfhost-frontend-authority" \
  root-host
TARGET_DIR="$CARGO_TARGET_DIR"

cargo build \
  --locked \
  --offline \
  -p gc_cli --bin genesis \
  -p gc_wasi_cli --bin genesis_wasi \
  >/dev/null

python3 scripts/lib/selfhost_frontend_authority.py \
  --root "$ROOT_DIR" \
  --profile policies/selfhost_frontend_authority_v0.1.json \
  --schema docs/spec/SELFHOST_FRONTEND_AUTHORITY_v0.1.schema.json \
  --self-test \
  --runtime \
  --genesis-bin "$TARGET_DIR/debug/genesis" \
  --genesis-wasi-bin "$TARGET_DIR/debug/genesis_wasi"
