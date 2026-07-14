#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPORT_PATH="${GENESIS_SELFHOST_DASHBOARD_FRESH_REPORT:-$ROOT_DIR/.genesis/perf/selfhost_dashboard_fresh_report.json}"
HISTORY_PATH="${GENESIS_SELFHOST_DASHBOARD_FRESH_HISTORY:-$ROOT_DIR/.genesis/perf/selfhost_dashboard_fresh_history.jsonl}"

exec bash "$ROOT_DIR/scripts/render_selfhost_dashboard_fresh_report.sh"   "$REPORT_PATH"   "$HISTORY_PATH"
