#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

PERSISTENT_REPORT_PATH="${GENESIS_STRESS_REPORT:-.genesis/perf/ai_stress_suite_metrics.json}"
PERSISTENT_HISTORY_PATH="${GENESIS_STRESS_HISTORY:-.genesis/perf/ai_stress_suite_history.jsonl}"
exec bash scripts/render_ai_stress_suite_report.sh \
  "$PERSISTENT_REPORT_PATH" \
  "$PERSISTENT_HISTORY_PATH" \
  "$PERSISTENT_HISTORY_PATH"
