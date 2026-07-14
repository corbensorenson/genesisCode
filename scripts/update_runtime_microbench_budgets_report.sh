#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

PERSISTENT_METRICS_PATH="${GENESIS_RUNTIME_MICROBENCH_OUT:-.genesis/perf/runtime_microbench_metrics.json}"
PERSISTENT_SLO_PATH="${GENESIS_CONCURRENCY_GPU_SLO_OUT:-.genesis/perf/concurrency_gpu_slo_report.json}"
PERSISTENT_RUNTIME_REPORT_PATH="${GENESIS_RUNTIME_MICROBENCH_RUNTIME_REPORT_OUT:-.genesis/perf/runtime_microbench_runtime_report.json}"
PERSISTENT_RUNTIME_HISTORY_PATH="${GENESIS_RUNTIME_MICROBENCH_RUNTIME_HISTORY_OUT:-.genesis/perf/runtime_microbench_runtime_history.jsonl}"
exec bash scripts/render_runtime_microbench_budgets_report.sh \
  "$PERSISTENT_METRICS_PATH" \
  "$PERSISTENT_SLO_PATH" \
  "$PERSISTENT_RUNTIME_REPORT_PATH" \
  "$PERSISTENT_RUNTIME_HISTORY_PATH" \
  "$PERSISTENT_METRICS_PATH" \
  "$PERSISTENT_RUNTIME_HISTORY_PATH"
