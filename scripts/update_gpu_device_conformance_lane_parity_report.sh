#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

LANE_A=""
LANE_B=""
OUT_PATH="${GENESIS_GPU_DEVICE_PARITY_REPORT_OUT:-.genesis/perf/gpu_device_lane_parity_report.json}"
usage() {
  echo "Usage: $0 --lane-a <report.json> --lane-b <report.json> [--out <report.json>]"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --lane-a) LANE_A="${2:-}"; shift 2 ;;
    --lane-b) LANE_B="${2:-}"; shift 2 ;;
    --out) OUT_PATH="${2:-}"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "gpu-device-lane-parity: unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
done

if [[ -z "$LANE_A" || -z "$LANE_B" || -z "$OUT_PATH" ]]; then
  echo "gpu-device-lane-parity: --lane-a, --lane-b, and a non-empty output are required" >&2
  usage >&2
  exit 2
fi

exec bash scripts/render_gpu_device_conformance_lane_parity_report.sh \
  "$LANE_A" \
  "$LANE_B" \
  "$OUT_PATH"
