#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

REPORT_PATH="${GENESIS_FULL_CROSS_HOST_PROFILE_REPORT:-.genesis/perf/full_cross_host_profile_report.json}"
HISTORY_PATH="${GENESIS_FULL_CROSS_HOST_PROFILE_HISTORY:-.genesis/perf/full_cross_host_profile_history.jsonl}"
STRICT_REPORT_INPUT="${GENESIS_STRICT_GOLDEN_PROFILE_REPORT:-.genesis/perf/strict_golden_profile_report.json}"
WASM_REPORT_INPUT="${GENESIS_WASM_CROSS_HOST_PROFILE_REPORT:-.genesis/perf/wasm_cross_host_profile_report.json}"

exec bash scripts/render_full_cross_host_profile_budget_report.sh \
  "$REPORT_PATH" \
  "$HISTORY_PATH" \
  "$HISTORY_PATH" \
  "$STRICT_REPORT_INPUT" \
  "$WASM_REPORT_INPUT"
