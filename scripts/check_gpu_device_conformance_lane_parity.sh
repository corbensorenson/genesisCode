#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

LANE_A=""
LANE_B=""
usage() {
  echo "Usage: $0 --lane-a <report.json> --lane-b <report.json>"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --lane-a) LANE_A="${2:-}"; shift 2 ;;
    --lane-b) LANE_B="${2:-}"; shift 2 ;;
    --out)
      echo "gpu-device-lane-parity: checks are read-only; use scripts/update_gpu_device_conformance_lane_parity_report.sh --out ${2:-<report.json>}" >&2
      exit 2
      ;;
    -h|--help) usage; exit 0 ;;
    *) echo "gpu-device-lane-parity: unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
done

if [[ -z "$LANE_A" || -z "$LANE_B" ]]; then
  echo "gpu-device-lane-parity: --lane-a and --lane-b are required" >&2
  usage >&2
  exit 2
fi

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT
bash scripts/render_gpu_device_conformance_lane_parity_report.sh \
  "$LANE_A" \
  "$LANE_B" \
  "$TMP_DIR/gpu_device_lane_parity_report.json"
