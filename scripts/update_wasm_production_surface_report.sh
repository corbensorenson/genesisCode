#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

PERSISTENT_REPORT_PATH="${GENESIS_WASM_PRODUCTION_SURFACE_REPORT:-.genesis/perf/wasm_production_surface_report.json}"
exec bash scripts/render_wasm_production_surface_report.sh "$PERSISTENT_REPORT_PATH"
