#!/usr/bin/env bash
set -euo pipefail

# Fast, high-signal test loop for local iteration.
# Full parity/cutover suites remain enforced separately (CI + strict scripts).

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

bash scripts/check_disk_headroom.sh --path "$ROOT_DIR" --context "test-fast"

SECONDS=0
if cargo nextest --version >/dev/null 2>&1; then
  RUNNER="nextest"
else
  RUNNER="cargo"
fi

echo "[test-fast] selfhost artifact freshness"
./scripts/check_selfhost_artifact_fresh.sh

if [[ "$RUNNER" == "nextest" ]]; then
  echo "[test-fast] cargo nextest run (core libs)"
  cargo nextest run \
    -p gc_coreform \
    -p gc_kernel \
    -p gc_prelude \
    -p gc_obligations \
    -p gc_patches \
    --profile ci

  echo "[test-fast] cargo nextest run (selected CLI integration tests)"
  cargo nextest run -p gc_cli \
    --test cli_smoke \
    --test cli_selfhost_only \
    --test cli_apply_patch_determinism \
    --test cli_typecheck_apply_patch_engine \
    --profile ci
else
  echo "[test-fast] cargo test (core libs)"
  cargo test \
    -p gc_coreform \
    -p gc_kernel \
    -p gc_prelude \
    -p gc_obligations \
    -p gc_patches

  echo "[test-fast] cargo test (selected CLI integration tests)"
  cargo test -p gc_cli --test cli_smoke
  cargo test -p gc_cli --test cli_selfhost_only
  cargo test -p gc_cli --test cli_apply_patch_determinism
  cargo test -p gc_cli --test cli_typecheck_apply_patch_engine
fi

echo "[test-fast] ok in ${SECONDS}s (runner=${RUNNER})"
