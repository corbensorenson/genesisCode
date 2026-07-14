#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

BASELINE_INPUT_FILE="${GENESIS_FUZZ_DIFFERENTIAL_HARDENING_HISTORY:-.genesis/perf/fuzz_differential_hardening_history.jsonl}"
TMP_DIR="$(mktemp -d)"
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

bash scripts/render_fuzz_differential_hardening_report.sh \
  "$TMP_DIR/fuzz_differential_hardening_report.json" \
  "$TMP_DIR/fuzz_differential_hardening_history.jsonl" \
  "$BASELINE_INPUT_FILE"
