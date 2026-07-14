#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

BASELINE_INPUT_FILE="${GENESIS_SELFHOST_SYMBOL_OWNERSHIP_HISTORY:-.genesis/perf/selfhost_symbol_ownership_history.jsonl}"
TMP_DIR="$(mktemp -d)"
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

bash scripts/render_selfhost_symbol_ownership_report.sh \
  "$TMP_DIR/selfhost_symbol_ownership_report.json" \
  "$TMP_DIR/selfhost_symbol_ownership_history.jsonl" \
  "$BASELINE_INPUT_FILE"
