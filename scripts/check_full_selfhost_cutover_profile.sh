#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

REFRESH_INPUTS="${GENESIS_FULL_SELFHOST_CUTOVER_REFRESH:-0}"
if [[ "$REFRESH_INPUTS" != "0" && "$REFRESH_INPUTS" != "1" ]]; then
  echo "full-selfhost-cutover-profile: GENESIS_FULL_SELFHOST_CUTOVER_REFRESH must be 0 or 1" >&2
  exit 2
fi
if [[ "$REFRESH_INPUTS" == "1" ]]; then
  echo "full-selfhost-cutover-profile: checks are read-only and cannot refresh prerequisite evidence." >&2
  echo "Run the explicit producers, inspect their outputs, then rerun with GENESIS_FULL_SELFHOST_CUTOVER_REFRESH=0:" >&2
  echo "  bash scripts/check_selfhost_boundary.sh --strict" >&2
  echo "  bash scripts/update_bootstrap_retirement_gate_report.sh" >&2
  echo "  bash scripts/update_selfhost_dashboard_fresh_report.sh" >&2
  echo "  bash scripts/update_selfhost_readiness_scorecard_report.sh" >&2
  echo "  bash scripts/update_kernel_tcb_contract_report.sh" >&2
  echo "  bash scripts/update_host_api_evolution_contract_report.sh" >&2
  echo "  bash scripts/update_gcpm_operation_contract_pack_report.sh" >&2
  echo "  bash scripts/update_vcs_selfhost_contract_report.sh" >&2
  echo "  bash scripts/update_selfhost_symbol_ownership_report.sh" >&2
  echo "  bash scripts/update_agent_generative_workloads_report.sh" >&2
  exit 2
fi

DOC_INPUT_FILE="docs/spec/FULL_SELFHOST_CUTOVER_PROFILE_v0.1.md"
READINESS_INPUT_FILE="${GENESIS_SELFHOST_READINESS_REPORT:-.genesis/perf/selfhost_readiness_report.json}"
BOOTSTRAP_INPUT_FILE="${GENESIS_BOOTSTRAP_RETIREMENT_REPORT:-.genesis/perf/bootstrap_retirement_gate_report.json}"
DASHBOARD_INPUT_FILE="${GENESIS_SELFHOST_DASHBOARD_FRESH_REPORT:-.genesis/perf/selfhost_dashboard_fresh_report.json}"
KERNEL_TCB_INPUT_FILE="${GENESIS_KERNEL_TCB_REPORT:-.genesis/perf/kernel_tcb_contract_report.json}"
HOST_API_INPUT_FILE="${GENESIS_HOST_API_EVOLUTION_REPORT:-.genesis/perf/host_api_evolution_contract_report.json}"
GCPM_INPUT_FILE="${GENESIS_GCPM_OPERATION_CONTRACT_PACK_REPORT:-.genesis/perf/gcpm_operation_contract_pack_report.json}"
VCS_INPUT_FILE="${GENESIS_VCS_SELFHOST_CONTRACT_REPORT:-.genesis/perf/vcs_selfhost_contract_report.json}"
SYMBOL_INPUT_FILE="${GENESIS_SELFHOST_SYMBOL_OWNERSHIP_REPORT:-.genesis/perf/selfhost_symbol_ownership_report.json}"
ARTIFACT_INPUT_FILE="${GENESIS_SELFHOST_TOOLCHAIN_ARTIFACT:-selfhost/toolchain.gc}"
GENERATIVE_INPUT_FILE="${GENESIS_AGENT_GENERATIVE_REPORT:-.genesis/perf/agent_generative_workloads_report.json}"

TMP_DIR="$(mktemp -d)"
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

bash scripts/render_full_selfhost_cutover_profile_report.sh \
  "$TMP_DIR/full_selfhost_cutover_profile_report.json" \
  "$TMP_DIR/full_selfhost_cutover_profile_history.jsonl" \
  "$DOC_INPUT_FILE" \
  "$READINESS_INPUT_FILE" \
  "$BOOTSTRAP_INPUT_FILE" \
  "$DASHBOARD_INPUT_FILE" \
  "$KERNEL_TCB_INPUT_FILE" \
  "$HOST_API_INPUT_FILE" \
  "$GCPM_INPUT_FILE" \
  "$VCS_INPUT_FILE" \
  "$SYMBOL_INPUT_FILE" \
  "$ARTIFACT_INPUT_FILE" \
  "$GENERATIVE_INPUT_FILE"
