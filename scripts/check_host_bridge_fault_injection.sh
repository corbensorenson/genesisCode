#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

BASELINE_FILE="${GENESIS_HOST_BRIDGE_FAULT_HISTORY:-.genesis/perf/host_bridge_fault_injection_history.jsonl}"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

bash scripts/render_host_bridge_fault_injection_report.sh \
  "$TMP_DIR/host_bridge_fault_injection_report.json" \
  "$TMP_DIR/host_bridge_fault_injection_history.jsonl" \
  "$BASELINE_FILE"
