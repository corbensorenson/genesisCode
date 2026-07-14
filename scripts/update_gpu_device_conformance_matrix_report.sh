#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

MATRIX_CONFIG="${GENESIS_GPU_DEVICE_MATRIX_CONFIG:-policies/perf/gpu_device_conformance_matrix.toml}"
OUT_PATH="${GENESIS_GPU_DEVICE_MATRIX_REPORT_OUT:-.genesis/perf/gpu_device_conformance_matrix_report.json}"
LANE_ARGS=()
usage() {
  echo "Usage: $0 [--config <matrix.toml>] [--out <report.json>] --lane <lane-id>=<report.json> [--lane ...]"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --config) MATRIX_CONFIG="${2:-}"; shift 2 ;;
    --out) OUT_PATH="${2:-}"; shift 2 ;;
    --lane) LANE_ARGS+=("${2:-}"); shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "gpu-device-matrix: unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
done

if [[ "${#LANE_ARGS[@]}" -eq 0 || -z "$OUT_PATH" ]]; then
  echo "gpu-device-matrix: at least one lane and a non-empty output are required" >&2
  usage >&2
  exit 2
fi

exec bash scripts/render_gpu_device_conformance_matrix_report.sh \
  "$MATRIX_CONFIG" \
  "$OUT_PATH" \
  "${LANE_ARGS[@]}"
