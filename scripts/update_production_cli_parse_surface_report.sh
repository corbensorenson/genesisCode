#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

PERSISTENT_REPORT_PATH="${GENESIS_PRODUCTION_CLI_PARSE_SURFACE_REPORT:-.genesis/perf/production_cli_parse_surface_report.json}"
PERSISTENT_HISTORY_PATH="${GENESIS_PRODUCTION_CLI_PARSE_SURFACE_HISTORY:-.genesis/perf/production_cli_parse_surface_history.jsonl}"
exec bash scripts/render_production_cli_parse_surface_report.sh \
  "$PERSISTENT_REPORT_PATH" "$PERSISTENT_HISTORY_PATH" "$PERSISTENT_HISTORY_PATH"
