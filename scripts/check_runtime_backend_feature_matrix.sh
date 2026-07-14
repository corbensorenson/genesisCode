#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

TIMING_BASELINE_FILE="${GENESIS_CHECK_RUNTIME_BACKEND_MATRIX_HISTORY_INPUT:-.genesis/perf/runtime_backend_feature_matrix_history.jsonl}"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

exec bash scripts/render_runtime_backend_feature_matrix_report.sh \
  "$TMP_DIR/runtime_backend_feature_matrix_report.json" \
  "$TMP_DIR/runtime_backend_feature_matrix_history.jsonl" \
  "$TIMING_BASELINE_FILE"
