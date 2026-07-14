#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPORT_OUT="${GENESIS_SOURCE_DECOMPOSITION_REPORT:-$ROOT_DIR/.genesis/perf/source_decomposition_progress_report.json}"

exec bash "$ROOT_DIR/scripts/render_source_decomposition_progress_report.sh" "$REPORT_OUT"
