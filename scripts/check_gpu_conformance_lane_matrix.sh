#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

CI_FILE=".github/workflows/ci.yml"
if [[ ! -f "$CI_FILE" ]]; then
  echo "gpu-conformance-lane-matrix: missing $CI_FILE" >&2
  exit 1
fi

require_pattern() {
  local pattern="$1"
  local message="$2"
  if ! rg -q --fixed-strings -- "$pattern" "$CI_FILE"; then
    echo "gpu-conformance-lane-matrix: $message" >&2
    exit 1
  fi
}

require_pattern "gpu_device_microbench:" "missing primary self-hosted gpu conformance lane job"
require_pattern "gpu_device_microbench_deterministic:" "missing deterministic secondary gpu conformance lane job"
require_pattern "gpu_device_conformance_release_gate:" "missing release conformance parity gate job"
require_pattern "gpu-device-conformance-artifacts-selfhosted-linux" "missing primary lane artifact upload"
require_pattern "gpu-device-conformance-artifacts-deterministic" "missing secondary lane artifact upload"
require_pattern "bash scripts/check_gpu_device_conformance_lane_parity.sh" "missing lane parity checker invocation"
require_pattern "needs:" "missing needs declaration for conformance parity gate"
require_pattern "- gpu_device_microbench" "conformance parity gate must require primary lane"
require_pattern "- gpu_device_microbench_deterministic" "conformance parity gate must require secondary lane"

echo "gpu-conformance-lane-matrix: ok"
