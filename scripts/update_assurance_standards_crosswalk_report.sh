#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPORT_PATH="${GENESIS_ASSURANCE_STANDARDS_CROSSWALK_REPORT:-$ROOT_DIR/.genesis/perf/assurance_standards_crosswalk_report.json}"
HISTORY_PATH="${GENESIS_ASSURANCE_STANDARDS_CROSSWALK_HISTORY:-$ROOT_DIR/.genesis/perf/assurance_standards_crosswalk_history.jsonl}"

exec bash "$ROOT_DIR/scripts/render_assurance_standards_crosswalk_report.sh"   "$REPORT_PATH"   "$HISTORY_PATH"
