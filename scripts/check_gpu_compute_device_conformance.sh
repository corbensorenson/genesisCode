#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

MICROBENCH_BASELINE_FILE="${GENESIS_RUNTIME_MICROBENCH_RUNTIME_HISTORY_OUT:-.genesis/perf/runtime_microbench_runtime_history.jsonl}"
COMPUTE_BASELINE_FILE="${GENESIS_GPU_COMPUTE_RUNTIME_PROFILE_RUNTIME_HISTORY_OUT:-.genesis/perf/gpu_compute_runtime_profile_runtime_history.jsonl}"
TMP_DIR="$(mktemp -d)"
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

bash scripts/render_gpu_compute_device_conformance_report.sh \
  "$TMP_DIR/gpu_device_conformance" \
  "$TMP_DIR/gpu_device_conformance_report.json" \
  "$MICROBENCH_BASELINE_FILE" \
  "$COMPUTE_BASELINE_FILE"
