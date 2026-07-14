#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

PERSISTENT_REPORT_PATH="${GENESIS_GPU_GFX_HEADROOM_REPORT_OUT:-.genesis/perf/gpu_gfx_headroom_conformance_report.json}"
PERSISTENT_HISTORY_PATH="${GENESIS_GPU_GFX_HEADROOM_HISTORY_OUT:-.genesis/perf/gpu_gfx_headroom_conformance_history.jsonl}"
DEVICE_INPUT="${GENESIS_GPU_GFX_HEADROOM_DEVICE_CONFORMANCE_REPORT:-.genesis/perf/gpu_device_conformance_report.json}"

exec bash scripts/render_gpu_gfx_headroom_conformance_report.sh \
  "$PERSISTENT_REPORT_PATH" \
  "$PERSISTENT_HISTORY_PATH" \
  "$PERSISTENT_REPORT_PATH" \
  "$PERSISTENT_HISTORY_PATH" \
  "$DEVICE_INPUT"
