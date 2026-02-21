#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

echo "runtime-backend-feature-matrix: checking gc_effects feature combinations"
cargo check -p gc_effects --quiet
cargo check -p gc_effects --no-default-features --features gpu-device-backend --quiet
cargo check -p gc_effects --no-default-features --features gfx-desktop-backend --quiet
cargo check -p gc_effects --no-default-features --features gpu-device-backend,gfx-desktop-backend --quiet

echo "runtime-backend-feature-matrix: checking gc_cli build profiles"
cargo check -p gc_cli --quiet

for profile in profile-headless profile-gpu profile-gfx profile-backend; do
  echo "runtime-backend-feature-matrix: gc_cli profile=${profile}"
  cargo check -p gc_cli --no-default-features --features "${profile}" --quiet
done

echo "runtime-backend-feature-matrix: checking gc_cli_driver backend profile tests"
for profile in profile-headless profile-gpu profile-gfx profile-backend; do
  echo "runtime-backend-feature-matrix: gc_cli_driver test profile=${profile}"
  cargo test -p gc_cli_driver --no-default-features --features "${profile}" \
    backend_feature_flags_match_active_profile --quiet
done

echo "runtime-backend-feature-matrix: ok"

