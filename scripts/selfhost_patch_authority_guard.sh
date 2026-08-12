#!/usr/bin/env bash
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"
source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
genesis_configure_cargo_target_dir "$ROOT_DIR" "selfhost-patch-authority" root-host
cargo build --locked --offline -p gc_cli --bin genesis -p gc_wasi_cli --bin genesis_wasi >/dev/null
python3 scripts/lib/selfhost_patch_authority.py --root "$ROOT_DIR" --profile policies/selfhost_patch_authority_v0.1.json --schema docs/spec/SELFHOST_PATCH_AUTHORITY_v0.1.schema.json --self-test --runtime --genesis-bin "$CARGO_TARGET_DIR/debug/genesis" --genesis-wasi-bin "$CARGO_TARGET_DIR/debug/genesis_wasi"
