#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

PERSISTENT_METRICS_PATH="${GENESIS_RUNTIME_WORKLOAD_OUT:-.genesis/perf/runtime_workload_bench_report.json}"
PERSISTENT_SAMPLE_HISTORY_PATH="${GENESIS_RUNTIME_WORKLOAD_HISTORY:-.genesis/perf/runtime_workload_bench_history.jsonl}"
PERSISTENT_RUNTIME_REPORT_PATH="${GENESIS_RUNTIME_WORKLOAD_RUNTIME_REPORT_OUT:-.genesis/perf/runtime_workload_bench_runtime_report.json}"
PERSISTENT_RUNTIME_HISTORY_PATH="${GENESIS_RUNTIME_WORKLOAD_RUNTIME_HISTORY_OUT:-.genesis/perf/runtime_workload_bench_runtime_history.jsonl}"
exec bash scripts/render_runtime_workload_budgets_report.sh \
  "$PERSISTENT_METRICS_PATH" \
  "$PERSISTENT_SAMPLE_HISTORY_PATH" \
  "$PERSISTENT_RUNTIME_REPORT_PATH" \
  "$PERSISTENT_RUNTIME_HISTORY_PATH" \
  "$PERSISTENT_METRICS_PATH" \
  "$PERSISTENT_SAMPLE_HISTORY_PATH" \
  "$PERSISTENT_RUNTIME_HISTORY_PATH"
