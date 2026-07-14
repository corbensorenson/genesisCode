#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

METRICS_BASELINE_FILE="${GENESIS_GPU_COMPUTE_RUNTIME_PROFILE_OUT:-.genesis/perf/gpu_compute_runtime_profile.json}"
TIMING_BASELINE_FILE="${GENESIS_GPU_COMPUTE_RUNTIME_PROFILE_RUNTIME_HISTORY_OUT:-.genesis/perf/gpu_compute_runtime_profile_runtime_history.jsonl}"
TMP_DIR="$(mktemp -d)"
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

bash scripts/render_gpu_compute_runtime_profile_report.sh \
  "$TMP_DIR/gpu_compute_runtime_profile.json" \
  "$TMP_DIR/gpu_compute_runtime_profile_guard.json" \
  "$TMP_DIR/gpu_compute_runtime_profile_runtime_report.json" \
  "$TMP_DIR/gpu_compute_runtime_profile_runtime_history.jsonl" \
  "$METRICS_BASELINE_FILE" \
  "$TIMING_BASELINE_FILE"
