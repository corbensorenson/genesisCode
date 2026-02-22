#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
source "$ROOT_DIR/scripts/lib/profile_gate_timing.sh"
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "check-cli-diagnostics-contract" \
  ".genesis/build/cargo" \
  "GENESIS_CHECK_CLI_DIAGNOSTICS_CONTRACT_CARGO_TARGET_DIR"

START_MS="$(genesis_profile_gate_now_ms)"
REPORT_PATH="${GENESIS_CLI_DIAGNOSTICS_CONTRACT_REPORT:-.genesis/perf/cli_diagnostics_contract_report.json}"
HISTORY_PATH="${GENESIS_CLI_DIAGNOSTICS_CONTRACT_HISTORY:-.genesis/perf/cli_diagnostics_contract_history.jsonl}"
BUDGET_MS="${GENESIS_CLI_DIAGNOSTICS_CONTRACT_BUDGET_MS:-300000}"

cargo test -p gc_cli --test cli_diagnostics_matrix --quiet

genesis_profile_gate_emit_runtime_report \
  "cli-diagnostics-contract" \
  "genesis/cli-diagnostics-contract-v0.1" \
  "$REPORT_PATH" \
  "$HISTORY_PATH" \
  "$START_MS" \
  "$BUDGET_MS"

echo "cli-diagnostics-contract: ok"
