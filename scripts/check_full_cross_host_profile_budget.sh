#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

STRICT_INPUT="${GENESIS_STRICT_GOLDEN_PROFILE_REPORT:-.genesis/perf/strict_golden_profile_report.json}"
WASM_INPUT="${GENESIS_WASM_CROSS_HOST_PROFILE_REPORT:-.genesis/perf/wasm_cross_host_profile_report.json}"
TIMING_BASELINE_FILE="${GENESIS_CHECK_FULL_CROSS_HOST_PROFILE_HISTORY_INPUT:-.genesis/perf/full_cross_host_profile_history.jsonl}"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

exec bash scripts/render_full_cross_host_profile_budget_report.sh \
  "$TMP_DIR/full_cross_host_profile_report.json" \
  "$TMP_DIR/full_cross_host_profile_history.jsonl" \
  "$TIMING_BASELINE_FILE" \
  "$STRICT_INPUT" \
  "$WASM_INPUT"
