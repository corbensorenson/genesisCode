#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

PERSISTENT_REPORT_PATH="${GENESIS_WEBXR_BROWSER_CONFORMANCE_OUT:-.genesis/perf/webxr_browser_conformance_report.json}"
exec bash scripts/render_webxr_browser_conformance_report.sh "$PERSISTENT_REPORT_PATH"
