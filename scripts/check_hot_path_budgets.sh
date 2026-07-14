#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

TIMING_BASELINE_FILE="${GENESIS_CHECK_HOT_PATH_RUNTIME_HISTORY_INPUT:-.genesis/perf/hot_path_runtime_history.jsonl}"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

bash scripts/render_hot_path_budgets_report.sh \
  "$TMP_DIR/hot_path_metrics.json" \
  "$TMP_DIR/hot_path_runtime_report.json" \
  "$TMP_DIR/hot_path_runtime_history.jsonl" \
  "$TIMING_BASELINE_FILE"
