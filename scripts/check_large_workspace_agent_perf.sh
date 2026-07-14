#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

METRICS_BASELINE_FILE="${GENESIS_CHECK_LARGE_WORKSPACE_METRICS_HISTORY_INPUT:-.genesis/perf/large_workspace_agent_perf_history.jsonl}"
TIMING_BASELINE_FILE="${GENESIS_CHECK_LARGE_WORKSPACE_RUNTIME_HISTORY_INPUT:-.genesis/perf/large_workspace_agent_runtime_history.jsonl}"
RUNTIME_SEED_FILE="${GENESIS_LARGE_WORKSPACE_RUNTIME_BASELINE_HISTORY:-}"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

exec bash scripts/render_large_workspace_agent_perf_report.sh \
  "$TMP_DIR/large_workspace_agent_perf_report.json" \
  "$TMP_DIR/large_workspace_agent_perf_history.jsonl" \
  "$TMP_DIR/large_workspace_agent_runtime_report.json" \
  "$TMP_DIR/large_workspace_agent_runtime_history.jsonl" \
  "$METRICS_BASELINE_FILE" \
  "$TIMING_BASELINE_FILE" \
  "$RUNTIME_SEED_FILE"
