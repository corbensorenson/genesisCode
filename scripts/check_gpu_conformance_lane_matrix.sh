#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

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
  if command -v rg >/dev/null 2>&1; then
    if rg -q --fixed-strings -- "$pattern" "$CI_FILE"; then
      return 0
    fi
  elif grep -Fq -- "$pattern" "$CI_FILE"; then
    return 0
  fi
  echo "gpu-conformance-lane-matrix: $message" >&2
  exit 1
}

require_pattern "gpu_device_microbench:" "missing primary self-hosted gpu conformance lane job"
require_pattern "gpu_device_microbench_deterministic:" "missing deterministic secondary gpu conformance lane job"
require_pattern "gpu_device_conformance_release_gate:" "missing release conformance parity gate job"
require_pattern "gpu-device-conformance-artifacts-selfhosted-linux" "missing primary lane artifact upload"
require_pattern "gpu-device-conformance-artifacts-deterministic" "missing secondary lane artifact upload"
require_pattern "bash scripts/update_gpu_compute_device_conformance_report.sh" "missing explicit gpu device conformance producer invocation"
require_pattern "bash scripts/update_gpu_device_conformance_lane_parity_report.sh" "missing explicit lane parity producer invocation"
require_pattern "needs:" "missing needs declaration for conformance parity gate"
require_pattern "- gpu_device_microbench" "conformance parity gate must require primary lane"
require_pattern "- gpu_device_microbench_deterministic" "conformance parity gate must require secondary lane"
require_pattern "gpu_device_microbench_nvidia_linux:" "missing NVIDIA Linux gpu conformance lane job"
require_pattern "gpu_device_microbench_amd_linux:" "missing AMD Linux gpu conformance lane job"
require_pattern "gpu_device_microbench_intel_windows:" "missing Intel Windows gpu conformance lane job"
require_pattern "gpu_device_microbench_apple_macos:" "missing Apple macOS gpu conformance lane job"
require_pattern "gpu_device_conformance_matrix_gate:" "missing multi-vendor gpu conformance matrix gate job"
require_pattern "gpu-device-conformance-artifacts-nvidia-linux" "missing NVIDIA Linux lane artifact upload"
require_pattern "gpu-device-conformance-artifacts-amd-linux" "missing AMD Linux lane artifact upload"
require_pattern "gpu-device-conformance-artifacts-intel-windows" "missing Intel Windows lane artifact upload"
require_pattern "gpu-device-conformance-artifacts-apple-macos" "missing Apple macOS lane artifact upload"
require_pattern "bash scripts/update_gpu_device_conformance_matrix_report.sh" "missing explicit gpu conformance matrix producer invocation"
require_pattern "--config policies/perf/gpu_device_conformance_matrix.toml" "missing matrix policy config for gpu conformance gate"

echo "gpu-conformance-lane-matrix: ok"
