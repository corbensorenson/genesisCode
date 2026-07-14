#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

PERSISTENT_METRICS_PATH="${GENESIS_GPU_COMPUTE_RUNTIME_PROFILE_OUT:-.genesis/perf/gpu_compute_runtime_profile.json}"
PERSISTENT_GUARD_PATH="${GENESIS_GPU_COMPUTE_RUNTIME_PROFILE_GUARD_OUT:-.genesis/perf/gpu_compute_runtime_profile_guard.json}"
PERSISTENT_RUNTIME_REPORT_PATH="${GENESIS_GPU_COMPUTE_RUNTIME_PROFILE_RUNTIME_REPORT_OUT:-.genesis/perf/gpu_compute_runtime_profile_runtime_report.json}"
PERSISTENT_RUNTIME_HISTORY_PATH="${GENESIS_GPU_COMPUTE_RUNTIME_PROFILE_RUNTIME_HISTORY_OUT:-.genesis/perf/gpu_compute_runtime_profile_runtime_history.jsonl}"
exec bash scripts/render_gpu_compute_runtime_profile_report.sh \
  "$PERSISTENT_METRICS_PATH" \
  "$PERSISTENT_GUARD_PATH" \
  "$PERSISTENT_RUNTIME_REPORT_PATH" \
  "$PERSISTENT_RUNTIME_HISTORY_PATH" \
  "$PERSISTENT_METRICS_PATH" \
  "$PERSISTENT_RUNTIME_HISTORY_PATH"
