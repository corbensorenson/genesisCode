#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

PERSISTENT_REPORT_PATH="${GENESIS_BACKEND_STARTER_REPORT:-.genesis/perf/backend_starter_workflows_report.json}"
PERSISTENT_HISTORY_PATH="${GENESIS_BACKEND_STARTER_HISTORY:-.genesis/perf/backend_starter_workflows_history.jsonl}"
exec bash scripts/render_backend_starter_workflows_report.sh \
  "$PERSISTENT_REPORT_PATH" \
  "$PERSISTENT_HISTORY_PATH"
