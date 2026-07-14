#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

PERSISTENT_PROFILE_PATH="${GENESIS_GFX_RUNTIME_PROFILE_OUT:-.genesis/perf/gfx_runtime_profile_report.json}"
PERSISTENT_RUNTIME_REPORT_PATH="${GENESIS_GFX_RUNTIME_PROFILE_RUNTIME_REPORT_OUT:-.genesis/perf/gfx_runtime_profile_runtime_report.json}"
PERSISTENT_RUNTIME_HISTORY_PATH="${GENESIS_GFX_RUNTIME_PROFILE_RUNTIME_HISTORY_OUT:-.genesis/perf/gfx_runtime_profile_runtime_history.jsonl}"

exec bash scripts/render_gfx_runtime_profile_report.sh \
  "$PERSISTENT_PROFILE_PATH" \
  "$PERSISTENT_RUNTIME_REPORT_PATH" \
  "$PERSISTENT_RUNTIME_HISTORY_PATH" \
  "$PERSISTENT_PROFILE_PATH" \
  "$PERSISTENT_RUNTIME_HISTORY_PATH"
