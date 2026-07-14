#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPORT_OUT="${GENESIS_TOOL_QUALIFICATION_LINEAGE_REPORT:-$ROOT_DIR/.genesis/perf/tool_qualification_lineage_report.json}"

exec bash "$ROOT_DIR/scripts/render_tool_qualification_lineage_report.sh" "$REPORT_OUT"
