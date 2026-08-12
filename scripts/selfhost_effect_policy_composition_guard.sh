#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"
source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
genesis_configure_cargo_target_dir "$ROOT_DIR" "selfhost-effect-policy-composition" root-host

artifact="${GENESIS_TEST_SELFHOST_ARTIFACT:-$ROOT_DIR/selfhost/toolchain.gc}"
python3 scripts/lib/selfhost_effect_policy_composition.py \
  --root "$ROOT_DIR" \
  --profile policies/selfhost_effect_policy_composition_v0.1.json \
  --schema docs/spec/SELFHOST_EFFECT_POLICY_COMPOSITION_v0.1.schema.json \
  --self-test
GENESIS_TEST_SELFHOST_ARTIFACT="$artifact" \
  cargo test -p gc_effects selfhost_authority_ --locked --offline -- --nocapture
