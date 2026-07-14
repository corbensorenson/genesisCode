#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

REPORT_PATH="${GENESIS_RUNTIME_BACKEND_MATRIX_REPORT_OUT:-.genesis/perf/runtime_backend_feature_matrix_report.json}"
HISTORY_PATH="${GENESIS_RUNTIME_BACKEND_MATRIX_HISTORY_OUT:-.genesis/perf/runtime_backend_feature_matrix_history.jsonl}"

exec bash scripts/render_runtime_backend_feature_matrix_report.sh \
  "$REPORT_PATH" \
  "$HISTORY_PATH" \
  "$HISTORY_PATH"
