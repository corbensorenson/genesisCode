#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

PERSISTENT_REPORT_PATH="${GENESIS_AI_ITERATION_SLO_OUT:-.genesis/perf/ai_iteration_slo_metrics.json}"
PERSISTENT_HISTORY_PATH="${GENESIS_AI_ITERATION_SLO_HISTORY:-.genesis/perf/ai_iteration_slo_history.jsonl}"
exec bash scripts/render_ai_iteration_slo_report.sh \
  "$PERSISTENT_REPORT_PATH" \
  "$PERSISTENT_HISTORY_PATH" \
  "$PERSISTENT_HISTORY_PATH"
