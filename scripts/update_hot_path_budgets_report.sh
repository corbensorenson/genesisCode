#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

PERSISTENT_METRICS_PATH="${GENESIS_HOT_PATH_METRICS_OUT:-.genesis/perf/hot_path_metrics.json}"
PERSISTENT_RUNTIME_REPORT_PATH="${GENESIS_HOT_PATH_RUNTIME_REPORT_OUT:-.genesis/perf/hot_path_runtime_report.json}"
PERSISTENT_RUNTIME_HISTORY_PATH="${GENESIS_HOT_PATH_RUNTIME_HISTORY_OUT:-.genesis/perf/hot_path_runtime_history.jsonl}"
exec bash scripts/render_hot_path_budgets_report.sh \
  "$PERSISTENT_METRICS_PATH" \
  "$PERSISTENT_RUNTIME_REPORT_PATH" \
  "$PERSISTENT_RUNTIME_HISTORY_PATH" \
  "$PERSISTENT_RUNTIME_HISTORY_PATH"
