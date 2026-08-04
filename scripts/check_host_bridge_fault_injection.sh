#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"
source "$ROOT_DIR/scripts/lib/health_profile_evidence.sh"

BASELINE_FILE="${GENESIS_HOST_BRIDGE_FAULT_HISTORY:-.genesis/perf/host_bridge_fault_injection_history.jsonl}"
PREBUILT_REPORT="${GENESIS_CHECK_HOST_BRIDGE_FAULT_REPORT:-}"
if [[ -n "$PREBUILT_REPORT" ]]; then
  genesis_verify_health_profile_evidence \
    "host-bridge-fault-injection" \
    "scripts/check_host_bridge_fault_injection.sh" \
    "$BASELINE_FILE" \
    "$PREBUILT_REPORT"
  exit 0
fi
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

bash scripts/render_host_bridge_fault_injection_report.sh \
  "$TMP_DIR/host_bridge_fault_injection_report.json" \
  "$TMP_DIR/host_bridge_fault_injection_history.jsonl" \
  "$BASELINE_FILE"
