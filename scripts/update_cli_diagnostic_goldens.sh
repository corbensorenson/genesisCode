#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

source scripts/lib/cargo_target_dir.sh
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "cli-diagnostic-goldens" \
  "root-host"

GENESIS_UPDATE_DIAGNOSTIC_GOLDENS=1 \
  cargo test -p gc_cli --test cli_diagnostic_goldens \
    diagnostic_failure_envelopes_match_versioned_goldens -- --exact

python3 scripts/lib/gc_diagnostic_goldens.py --check
