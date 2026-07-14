#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

PERSISTENT_REPORT_PATH="${GENESIS_HOST_BRIDGE_FAULT_REPORT:-.genesis/perf/host_bridge_fault_injection_report.json}"
PERSISTENT_HISTORY_PATH="${GENESIS_HOST_BRIDGE_FAULT_HISTORY:-.genesis/perf/host_bridge_fault_injection_history.jsonl}"
exec bash scripts/render_host_bridge_fault_injection_report.sh \
  "$PERSISTENT_REPORT_PATH" \
  "$PERSISTENT_HISTORY_PATH" \
  "$PERSISTENT_HISTORY_PATH"
