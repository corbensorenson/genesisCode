#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

PERSISTENT_REPORT_PATH="${GENESIS_AGENT_PARITY_REPORT:-.genesis/perf/agent_workflow_runtime_parity_report.json}"
PERSISTENT_HISTORY_PATH="${GENESIS_AGENT_PARITY_HISTORY:-.genesis/perf/agent_workflow_runtime_parity_history.jsonl}"
PERSISTENT_NATIVE_REPORT_PATH="${GENESIS_AGENT_PARITY_NATIVE_REPORT:-.genesis/perf/agent_capability_gauntlet_native_report.json}"
PERSISTENT_NATIVE_HISTORY_PATH="${GENESIS_AGENT_PARITY_NATIVE_HISTORY:-.genesis/perf/agent_capability_gauntlet_native_history.jsonl}"
PERSISTENT_WASI_REPORT_PATH="${GENESIS_AGENT_PARITY_WASI_REPORT:-.genesis/perf/agent_capability_gauntlet_wasi_report.json}"
PERSISTENT_WASI_HISTORY_PATH="${GENESIS_AGENT_PARITY_WASI_HISTORY:-.genesis/perf/agent_capability_gauntlet_wasi_history.jsonl}"
PERSISTENT_GENERATIVE_REPORT_PATH="${GENESIS_AGENT_PARITY_GENERATIVE_REPORT:-.genesis/perf/agent_generative_workloads_parity_report.json}"
PERSISTENT_GENERATIVE_HISTORY_PATH="${GENESIS_AGENT_PARITY_GENERATIVE_HISTORY:-.genesis/perf/agent_generative_workloads_parity_history.jsonl}"

exec bash scripts/render_agent_workflow_runtime_parity_report.sh \
  "$PERSISTENT_REPORT_PATH" \
  "$PERSISTENT_HISTORY_PATH" \
  "$PERSISTENT_HISTORY_PATH" \
  "$PERSISTENT_NATIVE_REPORT_PATH" \
  "$PERSISTENT_NATIVE_HISTORY_PATH" \
  "$PERSISTENT_NATIVE_REPORT_PATH" \
  "$PERSISTENT_NATIVE_HISTORY_PATH" \
  "$PERSISTENT_WASI_REPORT_PATH" \
  "$PERSISTENT_WASI_HISTORY_PATH" \
  "$PERSISTENT_WASI_REPORT_PATH" \
  "$PERSISTENT_WASI_HISTORY_PATH" \
  "$PERSISTENT_GENERATIVE_REPORT_PATH" \
  "$PERSISTENT_GENERATIVE_HISTORY_PATH" \
  "$PERSISTENT_GENERATIVE_HISTORY_PATH"
