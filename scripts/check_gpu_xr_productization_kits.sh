#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"
source "$ROOT_DIR/scripts/lib/health_profile_evidence.sh"

GAUNTLET_INPUT="${GENESIS_GPU_XR_PRODUCTIZATION_GAUNTLET_REPORT:-.genesis/perf/agent_capability_gauntlet_report.json}"
WEBXR_INPUT="${GENESIS_GPU_XR_PRODUCTIZATION_WEBXR_REPORT:-.genesis/perf/webxr_browser_conformance_report.json}"
AUTO_RUN_GAUNTLET="${GENESIS_GPU_XR_PRODUCTIZATION_AUTO_RUN_GAUNTLET:-0}"
REQUIRE_WEBXR="${GENESIS_GPU_XR_REQUIRE_WEBXR_RUNTIME_EVIDENCE:-1}"
PREBUILT_REPORT="${GENESIS_GPU_XR_PRODUCTIZATION_PREBUILT_REPORT:-}"

[[ "$AUTO_RUN_GAUNTLET" == "0" || "$AUTO_RUN_GAUNTLET" == "1" ]] || {
  echo "gpu-xr-productization-kits: GENESIS_GPU_XR_PRODUCTIZATION_AUTO_RUN_GAUNTLET must be 0 or 1" >&2
  exit 2
}
[[ "$REQUIRE_WEBXR" == "0" || "$REQUIRE_WEBXR" == "1" ]] || {
  echo "gpu-xr-productization-kits: GENESIS_GPU_XR_REQUIRE_WEBXR_RUNTIME_EVIDENCE must be 0 or 1" >&2
  exit 2
}

if [[ -n "$PREBUILT_REPORT" ]]; then
  genesis_verify_health_profile_evidence \
    "gpu-xr-productization" \
    "scripts/check_gpu_xr_productization_kits.sh" \
    "$GAUNTLET_INPUT" \
    "$PREBUILT_REPORT" \
    "$WEBXR_INPUT"
  exit 0
fi

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

EFFECTIVE_GAUNTLET="$GAUNTLET_INPUT"
if [[ ! -f "$EFFECTIVE_GAUNTLET" ]]; then
  if [[ "$AUTO_RUN_GAUNTLET" != "1" ]]; then
    echo "gpu-xr-productization-kits: missing gauntlet input: $EFFECTIVE_GAUNTLET" >&2
    echo "gpu-xr-productization-kits: produce it with: bash scripts/update_agent_reference_workflows_report.sh" >&2
    exit 1
  fi
  EFFECTIVE_GAUNTLET="$TMP_DIR/agent_capability_gauntlet_report.json"
  GENESIS_AGENT_GAUNTLET_PROFILE="${GENESIS_AGENT_GAUNTLET_PROFILE:-prepush-standard}" \
    bash scripts/render_agent_reference_workflows_report.sh \
      "$EFFECTIVE_GAUNTLET" \
      "$TMP_DIR/agent_capability_gauntlet_history.jsonl" \
      "$GAUNTLET_INPUT"
fi

EFFECTIVE_WEBXR="$WEBXR_INPUT"
if [[ "$REQUIRE_WEBXR" == "1" && ! -f "$EFFECTIVE_WEBXR" ]]; then
  EFFECTIVE_WEBXR="$TMP_DIR/webxr_browser_conformance_report.json"
  bash scripts/render_webxr_browser_conformance_report.sh "$EFFECTIVE_WEBXR"
fi

bash scripts/render_gpu_xr_productization_kits_report.sh \
  "$TMP_DIR/gpu_xr_productization_kits_report.json" \
  "$EFFECTIVE_GAUNTLET" \
  "$EFFECTIVE_WEBXR"
