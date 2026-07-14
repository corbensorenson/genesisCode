#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

BASELINE_METRICS_FILE="${GENESIS_GPU_GFX_HEADROOM_REPORT_INPUT:-.genesis/perf/gpu_gfx_headroom_conformance_report.json}"
BASELINE_TIMING_FILE="${GENESIS_GPU_GFX_HEADROOM_HISTORY_INPUT:-.genesis/perf/gpu_gfx_headroom_conformance_history.jsonl}"
DEVICE_EVIDENCE_FILE="${GENESIS_GPU_GFX_HEADROOM_DEVICE_CONFORMANCE_REPORT:-.genesis/perf/gpu_device_conformance_report.json}"
DEVICE_REFRESH="${GENESIS_GPU_GFX_HEADROOM_DEVICE_CONFORMANCE_REFRESH:-0}"

if [[ "$DEVICE_REFRESH" != "0" && "$DEVICE_REFRESH" != "1" ]]; then
  echo "gpu-gfx-headroom: GENESIS_GPU_GFX_HEADROOM_DEVICE_CONFORMANCE_REFRESH must be 0 or 1" >&2
  exit 2
fi
if [[ "$DEVICE_REFRESH" == "1" ]]; then
  echo "gpu-gfx-headroom: checks are read-only; produce device evidence first with: bash scripts/update_gpu_compute_device_conformance_report.sh" >&2
  exit 2
fi

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT
GENESIS_GPU_GFX_HEADROOM_TMPDIR="$TMP_DIR/runtime" \
  bash scripts/render_gpu_gfx_headroom_conformance_report.sh \
    "$TMP_DIR/gpu_gfx_headroom_conformance_report.json" \
    "$TMP_DIR/gpu_gfx_headroom_conformance_history.jsonl" \
    "$BASELINE_METRICS_FILE" \
    "$BASELINE_TIMING_FILE" \
    "$DEVICE_EVIDENCE_FILE"
