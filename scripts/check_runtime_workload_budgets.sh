#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

METRICS_BASELINE_FILE="${GENESIS_CHECK_RUNTIME_WORKLOAD_METRICS_INPUT:-.genesis/perf/runtime_workload_bench_report.json}"
SAMPLE_BASELINE_FILE="${GENESIS_CHECK_RUNTIME_WORKLOAD_METRICS_HISTORY_INPUT:-.genesis/perf/runtime_workload_bench_history.jsonl}"
TIMING_BASELINE_FILE="${GENESIS_CHECK_RUNTIME_WORKLOAD_RUNTIME_HISTORY_INPUT:-.genesis/perf/runtime_workload_bench_runtime_history.jsonl}"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

bash scripts/render_runtime_workload_budgets_report.sh \
  "$TMP_DIR/runtime_workload_bench_report.json" \
  "$TMP_DIR/runtime_workload_bench_history.jsonl" \
  "$TMP_DIR/runtime_workload_bench_runtime_report.json" \
  "$TMP_DIR/runtime_workload_bench_runtime_history.jsonl" \
  "$METRICS_BASELINE_FILE" \
  "$SAMPLE_BASELINE_FILE" \
  "$TIMING_BASELINE_FILE"
