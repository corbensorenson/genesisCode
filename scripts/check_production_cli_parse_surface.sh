#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

BASELINE_INPUT_FILE="${GENESIS_PRODUCTION_CLI_PARSE_SURFACE_HISTORY:-.genesis/perf/production_cli_parse_surface_history.jsonl}"
TMP_DIR="$(mktemp -d)"
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

bash scripts/render_production_cli_parse_surface_report.sh \
  "$TMP_DIR/production_cli_parse_surface_report.json" \
  "$TMP_DIR/production_cli_parse_surface_history.jsonl" \
  "$BASELINE_INPUT_FILE"
