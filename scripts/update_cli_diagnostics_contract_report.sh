#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

PERSISTENT_REPORT_PATH="${GENESIS_CLI_DIAGNOSTICS_CONTRACT_REPORT:-.genesis/perf/cli_diagnostics_contract_report.json}"
PERSISTENT_HISTORY_PATH="${GENESIS_CLI_DIAGNOSTICS_CONTRACT_HISTORY:-.genesis/perf/cli_diagnostics_contract_history.jsonl}"
bash scripts/update_gc_diagnostic_catalog.sh
bash scripts/update_gc_repair_utility_report.sh
exec bash scripts/render_cli_diagnostics_contract_report.sh \
  "$PERSISTENT_REPORT_PATH" "$PERSISTENT_HISTORY_PATH" "$PERSISTENT_HISTORY_PATH"
