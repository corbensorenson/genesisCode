#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"
source "$ROOT_DIR/scripts/lib/health_profile_evidence.sh"

DEFAULT_PRIMARY_INPUT=".genesis/perf/agent_capability_gauntlet_native_report.json"
if [[ ! -f "$DEFAULT_PRIMARY_INPUT" ]]; then
  DEFAULT_PRIMARY_INPUT=".genesis/perf/agent_capability_gauntlet_report.json"
fi
PRIMARY_INPUT_FILE="${GENESIS_AGENT_GENERATIVE_PRIMARY_REPORT:-$DEFAULT_PRIMARY_INPUT}"
REQUIRE_SECONDARY="${GENESIS_AGENT_GENERATIVE_REQUIRE_SECONDARY:-1}"
DEFAULT_SECONDARY_INPUT=".genesis/perf/agent_capability_gauntlet_wasi_report.json"
if [[ -n "${GENESIS_AGENT_GENERATIVE_SECONDARY_REPORT:-}" ]]; then
  SECONDARY_INPUT_FILE="$GENESIS_AGENT_GENERATIVE_SECONDARY_REPORT"
elif [[ "$REQUIRE_SECONDARY" == "1" && -f "$DEFAULT_SECONDARY_INPUT" ]]; then
  SECONDARY_INPUT_FILE="$DEFAULT_SECONDARY_INPUT"
else
  SECONDARY_INPUT_FILE=""
fi
BASELINE_INPUT_FILE="${GENESIS_AGENT_GENERATIVE_HISTORY:-.genesis/perf/agent_generative_workloads_history.jsonl}"
PREBUILT_REPORT="${GENESIS_AGENT_GENERATIVE_PREBUILT_REPORT:-}"

if [[ -n "$PREBUILT_REPORT" ]]; then
  genesis_verify_health_profile_evidence \
    "agent-generative-workloads" \
    "scripts/check_agent_generative_workloads.sh" \
    "$PRIMARY_INPUT_FILE" \
    "$SECONDARY_INPUT_FILE" \
    "$BASELINE_INPUT_FILE" \
    "$PREBUILT_REPORT"
  exit 0
fi

TMP_DIR="$(mktemp -d)"
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

bash scripts/render_agent_generative_workloads_report.sh \
  "$TMP_DIR/agent_generative_workloads_report.json" \
  "$TMP_DIR/agent_generative_workloads_history.jsonl" \
  "$BASELINE_INPUT_FILE" \
  "$PRIMARY_INPUT_FILE" \
  "$SECONDARY_INPUT_FILE"
