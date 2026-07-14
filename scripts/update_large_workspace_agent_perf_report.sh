#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

METRICS_PATH="${GENESIS_LARGE_WORKSPACE_REPORT_OUT:-.genesis/perf/large_workspace_agent_perf_report.json}"
METRICS_HISTORY_PATH="${GENESIS_LARGE_WORKSPACE_METRICS_HISTORY:-.genesis/perf/large_workspace_agent_perf_history.jsonl}"
RUNTIME_REPORT_PATH="${GENESIS_LARGE_WORKSPACE_RUNTIME_REPORT:-.genesis/perf/large_workspace_agent_runtime_report.json}"
RUNTIME_HISTORY_PATH="${GENESIS_LARGE_WORKSPACE_RUNTIME_HISTORY:-.genesis/perf/large_workspace_agent_runtime_history.jsonl}"
RUNTIME_SEED_FILE="${GENESIS_LARGE_WORKSPACE_RUNTIME_BASELINE_HISTORY:-}"

exec bash scripts/render_large_workspace_agent_perf_report.sh \
  "$METRICS_PATH" \
  "$METRICS_HISTORY_PATH" \
  "$RUNTIME_REPORT_PATH" \
  "$RUNTIME_HISTORY_PATH" \
  "$METRICS_HISTORY_PATH" \
  "$RUNTIME_HISTORY_PATH" \
  "$RUNTIME_SEED_FILE"
