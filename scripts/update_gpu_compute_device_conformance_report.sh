#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

PERSISTENT_ARTIFACT_DIR="${GENESIS_GPU_DEVICE_CONFORMANCE_OUT_DIR:-.genesis/perf/gpu_device_conformance}"
PERSISTENT_REPORT_PATH="${GENESIS_GPU_DEVICE_CONFORMANCE_REPORT_OUT:-.genesis/perf/gpu_device_conformance_report.json}"
MICROBENCH_BASELINE_FILE="${GENESIS_RUNTIME_MICROBENCH_RUNTIME_HISTORY_OUT:-.genesis/perf/runtime_microbench_runtime_history.jsonl}"
COMPUTE_BASELINE_FILE="${GENESIS_GPU_COMPUTE_RUNTIME_PROFILE_RUNTIME_HISTORY_OUT:-.genesis/perf/gpu_compute_runtime_profile_runtime_history.jsonl}"
exec bash scripts/render_gpu_compute_device_conformance_report.sh \
  "$PERSISTENT_ARTIFACT_DIR" \
  "$PERSISTENT_REPORT_PATH" \
  "$MICROBENCH_BASELINE_FILE" \
  "$COMPUTE_BASELINE_FILE"
