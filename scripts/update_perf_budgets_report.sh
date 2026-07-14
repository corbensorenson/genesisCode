#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

PERSISTENT_METRICS_PATH="${GENESIS_PERF_BUDGET_REPORT_OUT:-.genesis/perf/perf_budget_metrics.json}"
PERSISTENT_RUNTIME_REPORT_PATH="${GENESIS_PERF_BUDGET_RUNTIME_REPORT_OUT:-.genesis/perf/perf_budget_runtime_report.json}"
PERSISTENT_RUNTIME_HISTORY_PATH="${GENESIS_PERF_BUDGET_HISTORY_OUT:-.genesis/perf/perf_budget_metrics_history.jsonl}"
exec bash scripts/render_perf_budgets_report.sh \
  "$PERSISTENT_METRICS_PATH" \
  "$PERSISTENT_RUNTIME_REPORT_PATH" \
  "$PERSISTENT_RUNTIME_HISTORY_PATH" \
  "$PERSISTENT_RUNTIME_HISTORY_PATH"
