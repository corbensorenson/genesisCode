#!/usr/bin/env bash
set -euo pipefail

# Fast, high-signal test loop for local iteration.
# Full parity/cutover suites remain enforced separately (CI + strict scripts).

SECONDS=0

echo "[test-fast] selfhost artifact freshness"
./scripts/check_selfhost_artifact_fresh.sh

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

echo "[test-fast] ok in ${SECONDS}s"
