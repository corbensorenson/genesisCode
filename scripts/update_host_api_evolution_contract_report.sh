#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPORT_OUT="${GENESIS_HOST_API_EVOLUTION_REPORT:-$ROOT_DIR/.genesis/perf/host_api_evolution_contract_report.json}"

exec bash "$ROOT_DIR/scripts/render_host_api_evolution_contract_report.sh" "$REPORT_OUT"
