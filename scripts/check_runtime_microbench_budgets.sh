#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

METRICS_BASELINE_FILE="${GENESIS_RUNTIME_MICROBENCH_OUT:-.genesis/perf/runtime_microbench_metrics.json}"
TIMING_BASELINE_FILE="${GENESIS_RUNTIME_MICROBENCH_RUNTIME_HISTORY_OUT:-.genesis/perf/runtime_microbench_runtime_history.jsonl}"
TMP_DIR="$(mktemp -d)"
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

bash scripts/render_runtime_microbench_budgets_report.sh \
  "$TMP_DIR/runtime_microbench_metrics.json" \
  "$TMP_DIR/concurrency_gpu_slo_report.json" \
  "$TMP_DIR/runtime_microbench_runtime_report.json" \
  "$TMP_DIR/runtime_microbench_runtime_history.jsonl" \
  "$METRICS_BASELINE_FILE" \
  "$TIMING_BASELINE_FILE"
