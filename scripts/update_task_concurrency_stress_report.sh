#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

PERSISTENT_REPORT_PATH="${GENESIS_TASK_STRESS_REPORT:-.genesis/perf/task_concurrency_stress_report.json}"
PERSISTENT_HISTORY_PATH="${GENESIS_TASK_STRESS_HISTORY:-.genesis/perf/task_concurrency_stress_history.jsonl}"
exec bash scripts/render_task_concurrency_stress_report.sh \
  "$PERSISTENT_REPORT_PATH" \
  "$PERSISTENT_HISTORY_PATH" \
  "$PERSISTENT_HISTORY_PATH"
