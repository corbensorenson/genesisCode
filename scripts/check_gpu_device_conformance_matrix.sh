#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

MATRIX_CONFIG="${GENESIS_GPU_DEVICE_MATRIX_CONFIG:-policies/perf/gpu_device_conformance_matrix.toml}"
LANE_ARGS=()
usage() {
  echo "Usage: $0 [--config <matrix.toml>] --lane <lane-id>=<report.json> [--lane ...]"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --config) MATRIX_CONFIG="${2:-}"; shift 2 ;;
    --lane) LANE_ARGS+=("${2:-}"); shift 2 ;;
    --out)
      echo "gpu-device-matrix: checks are read-only; use scripts/update_gpu_device_conformance_matrix_report.sh --out ${2:-<report.json>}" >&2
      exit 2
      ;;
    -h|--help) usage; exit 0 ;;
    *) echo "gpu-device-matrix: unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
done

if [[ "${#LANE_ARGS[@]}" -eq 0 ]]; then
  echo "gpu-device-matrix: at least one --lane <id>=<report.json> is required" >&2
  usage >&2
  exit 2
fi

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT
bash scripts/render_gpu_device_conformance_matrix_report.sh \
  "$MATRIX_CONFIG" \
  "$TMP_DIR/gpu_device_conformance_matrix_report.json" \
  "${LANE_ARGS[@]}"
