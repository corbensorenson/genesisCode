#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

# boundary: dynamic-compilation-subject (runtime-profile renderer builds benchmark subjects)

PROFILE_INPUT_FILE="${GENESIS_GFX_RUNTIME_PROFILE_OUT:-.genesis/perf/gfx_runtime_profile_report.json}"
RUNTIME_BASELINE_FILE="${GENESIS_GFX_RUNTIME_PROFILE_RUNTIME_HISTORY_OUT:-.genesis/perf/gfx_runtime_profile_runtime_history.jsonl}"
TMP_DIR="$(mktemp -d)"
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

bash scripts/render_gfx_runtime_profile_report.sh \
  "$TMP_DIR/gfx_runtime_profile_report.json" \
  "$TMP_DIR/gfx_runtime_profile_runtime_report.json" \
  "$TMP_DIR/gfx_runtime_profile_runtime_history.jsonl" \
  "$PROFILE_INPUT_FILE" \
  "$RUNTIME_BASELINE_FILE"
