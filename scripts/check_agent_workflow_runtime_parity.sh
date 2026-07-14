#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

TOP_BASELINE_FILE="${GENESIS_AGENT_PARITY_HISTORY:-.genesis/perf/agent_workflow_runtime_parity_history.jsonl}"
NATIVE_EVIDENCE_FILE="${GENESIS_AGENT_PARITY_NATIVE_REPORT:-.genesis/perf/agent_capability_gauntlet_native_report.json}"
NATIVE_TIMING_FILE="${GENESIS_AGENT_PARITY_NATIVE_HISTORY:-.genesis/perf/agent_capability_gauntlet_native_history.jsonl}"
WASI_EVIDENCE_FILE="${GENESIS_AGENT_PARITY_WASI_REPORT:-.genesis/perf/agent_capability_gauntlet_wasi_report.json}"
WASI_TIMING_FILE="${GENESIS_AGENT_PARITY_WASI_HISTORY:-.genesis/perf/agent_capability_gauntlet_wasi_history.jsonl}"
GENERATIVE_BASELINE_FILE="${GENESIS_AGENT_PARITY_GENERATIVE_HISTORY:-.genesis/perf/agent_generative_workloads_parity_history.jsonl}"

TMP_DIR="$(mktemp -d)"
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

bash scripts/render_agent_workflow_runtime_parity_report.sh \
  "$TMP_DIR/agent_workflow_runtime_parity_report.json" \
  "$TMP_DIR/agent_workflow_runtime_parity_history.jsonl" \
  "$TOP_BASELINE_FILE" \
  "$TMP_DIR/agent_capability_gauntlet_native_report.json" \
  "$TMP_DIR/agent_capability_gauntlet_native_history.jsonl" \
  "$NATIVE_EVIDENCE_FILE" \
  "$NATIVE_TIMING_FILE" \
  "$TMP_DIR/agent_capability_gauntlet_wasi_report.json" \
  "$TMP_DIR/agent_capability_gauntlet_wasi_history.jsonl" \
  "$WASI_EVIDENCE_FILE" \
  "$WASI_TIMING_FILE" \
  "$TMP_DIR/agent_generative_workloads_parity_report.json" \
  "$TMP_DIR/agent_generative_workloads_parity_history.jsonl" \
  "$GENERATIVE_BASELINE_FILE"
