#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"
source "$ROOT_DIR/scripts/lib/health_profile_evidence.sh"

GAUNTLET_EVIDENCE_FILE="${GENESIS_AGENT_GAUNTLET_REPORT:-.genesis/perf/agent_capability_gauntlet_report.json}"
GAUNTLET_TIMING_FILE="${GENESIS_AGENT_GAUNTLET_HISTORY:-.genesis/perf/agent_capability_gauntlet_history.jsonl}"
if [[ -n "${GENESIS_HEALTH_EVIDENCE_MANIFEST:-}" ]]; then
  genesis_verify_health_profile_evidence \
    "agent-scenario-performance" \
    "scripts/check_agent_scenario_perf.sh" \
    "$GAUNTLET_TIMING_FILE" \
    "$GAUNTLET_EVIDENCE_FILE"
fi
TMP_DIR="$(mktemp -d)"
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

bash scripts/render_agent_scenario_perf_report.sh \
  "$TMP_DIR/agent_scenario_perf_report.json" \
  "$TMP_DIR/agent_scenario_perf_history.jsonl" \
  "$GAUNTLET_EVIDENCE_FILE" \
  "$GAUNTLET_TIMING_FILE"
