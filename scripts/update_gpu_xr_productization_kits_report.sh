#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

GAUNTLET_INPUT="${GENESIS_GPU_XR_PRODUCTIZATION_GAUNTLET_REPORT:-.genesis/perf/agent_capability_gauntlet_report.json}"
GAUNTLET_HISTORY="${GENESIS_GPU_XR_PRODUCTIZATION_GAUNTLET_HISTORY:-.genesis/perf/agent_capability_gauntlet_history.jsonl}"
WEBXR_INPUT="${GENESIS_GPU_XR_PRODUCTIZATION_WEBXR_REPORT:-.genesis/perf/webxr_browser_conformance_report.json}"
PERSISTENT_REPORT_PATH="${GENESIS_GPU_XR_PRODUCTIZATION_REPORT:-.genesis/perf/gpu_xr_productization_kits_report.json}"
AUTO_RUN_GAUNTLET="${GENESIS_GPU_XR_PRODUCTIZATION_AUTO_RUN_GAUNTLET:-0}"
REQUIRE_WEBXR="${GENESIS_GPU_XR_REQUIRE_WEBXR_RUNTIME_EVIDENCE:-1}"

[[ "$AUTO_RUN_GAUNTLET" == "0" || "$AUTO_RUN_GAUNTLET" == "1" ]] || {
  echo "gpu-xr-productization-kits: GENESIS_GPU_XR_PRODUCTIZATION_AUTO_RUN_GAUNTLET must be 0 or 1" >&2
  exit 2
}
[[ "$REQUIRE_WEBXR" == "0" || "$REQUIRE_WEBXR" == "1" ]] || {
  echo "gpu-xr-productization-kits: GENESIS_GPU_XR_REQUIRE_WEBXR_RUNTIME_EVIDENCE must be 0 or 1" >&2
  exit 2
}

if [[ ! -f "$GAUNTLET_INPUT" ]]; then
  if [[ "$AUTO_RUN_GAUNTLET" != "1" ]]; then
    echo "gpu-xr-productization-kits: missing gauntlet input: $GAUNTLET_INPUT" >&2
    echo "gpu-xr-productization-kits: produce it with: bash scripts/update_agent_reference_workflows_report.sh" >&2
    exit 1
  fi
  GENESIS_AGENT_GAUNTLET_PROFILE="${GENESIS_AGENT_GAUNTLET_PROFILE:-prepush-standard}" \
  GENESIS_AGENT_GAUNTLET_REPORT="$GAUNTLET_INPUT" \
  GENESIS_AGENT_GAUNTLET_HISTORY="$GAUNTLET_HISTORY" \
    bash scripts/update_agent_reference_workflows_report.sh
fi

if [[ "$REQUIRE_WEBXR" == "1" && ! -f "$WEBXR_INPUT" ]]; then
  if [[ "$AUTO_RUN_GAUNTLET" != "1" ]]; then
    echo "gpu-xr-productization-kits: missing WebXR input: $WEBXR_INPUT" >&2
    echo "gpu-xr-productization-kits: produce it with: bash scripts/update_webxr_browser_conformance_report.sh" >&2
    exit 1
  fi
  GENESIS_WEBXR_BROWSER_CONFORMANCE_OUT="$WEBXR_INPUT" \
    bash scripts/update_webxr_browser_conformance_report.sh
fi

exec bash scripts/render_gpu_xr_productization_kits_report.sh \
  "$PERSISTENT_REPORT_PATH" \
  "$GAUNTLET_INPUT" \
  "$WEBXR_INPUT"
