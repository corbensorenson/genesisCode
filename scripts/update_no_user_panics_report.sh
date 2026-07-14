#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPORT_PATH="${GENESIS_NO_USER_PANICS_REPORT:-$ROOT_DIR/.genesis/perf/no_user_panics_report.json}"
HISTORY_PATH="${GENESIS_NO_USER_PANICS_HISTORY:-$ROOT_DIR/.genesis/perf/no_user_panics_history.jsonl}"

exec bash "$ROOT_DIR/scripts/render_no_user_panics_report.sh"   "$REPORT_PATH"   "$HISTORY_PATH"
