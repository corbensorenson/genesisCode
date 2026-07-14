#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

BASELINE_INPUT_FILE="${GENESIS_FOUNDATION_STDLIB_CONFORMANCE_HISTORY:-.genesis/perf/foundation_stdlib_conformance_history.jsonl}"
TMP_DIR="$(mktemp -d)"
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

bash scripts/render_foundation_stdlib_conformance_report.sh \
  "$TMP_DIR/foundation_stdlib_conformance_report.json" \
  "$TMP_DIR/foundation_stdlib_conformance_history.jsonl" \
  "$BASELINE_INPUT_FILE"
