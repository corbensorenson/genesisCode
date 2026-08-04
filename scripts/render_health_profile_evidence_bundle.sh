#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if [[ "$#" -ne 2 ]]; then
  echo "usage: $0 <prepush-standard|release-full> <output-root>" >&2
  exit 2
fi

PROFILE="$1"
OUTPUT_ROOT="$2"
case "$PROFILE" in
  prepush-standard|release-full) ;;
  *)
    echo "health-profile-evidence: unsupported profile: $PROFILE" >&2
    exit 2
    ;;
esac

mkdir -p "$OUTPUT_ROOT"
INPUT_ROOT="$OUTPUT_ROOT/baseline-inputs"

GAUNTLET_REPORT="$OUTPUT_ROOT/agent_capability_gauntlet_report.json"
GAUNTLET_HISTORY="$OUTPUT_ROOT/agent_capability_gauntlet_history.jsonl"
RUNTIME_BACKEND_REPORT="$OUTPUT_ROOT/runtime_backend_feature_matrix_report.json"
RUNTIME_BACKEND_HISTORY="$OUTPUT_ROOT/runtime_backend_feature_matrix_history.jsonl"
HOST_BRIDGE_REPORT="$OUTPUT_ROOT/host_bridge_fault_injection_report.json"
HOST_BRIDGE_HISTORY="$OUTPUT_ROOT/host_bridge_fault_injection_history.jsonl"
WEBXR_REPORT="$OUTPUT_ROOT/webxr_browser_conformance_report.json"
GPU_XR_REPORT="$OUTPUT_ROOT/gpu_xr_productization_kits_report.json"
ASSURANCE_REPORT="$OUTPUT_ROOT/assurance_profile_packs_report.json"
ASSURANCE_HISTORY="$OUTPUT_ROOT/assurance_profile_packs_history.jsonl"
PARITY_REPORT="$OUTPUT_ROOT/agent_workflow_runtime_parity_report.json"
PARITY_HISTORY="$OUTPUT_ROOT/agent_workflow_runtime_parity_history.jsonl"
NATIVE_REPORT="$OUTPUT_ROOT/agent_capability_gauntlet_native_report.json"
NATIVE_HISTORY="$OUTPUT_ROOT/agent_capability_gauntlet_native_history.jsonl"
WASI_REPORT="$OUTPUT_ROOT/agent_capability_gauntlet_wasi_report.json"
WASI_HISTORY="$OUTPUT_ROOT/agent_capability_gauntlet_wasi_history.jsonl"
GENERATIVE_REPORT="$OUTPUT_ROOT/agent_generative_workloads_report.json"
GENERATIVE_HISTORY="$OUTPUT_ROOT/agent_generative_workloads_history.jsonl"

# Inputs deliberately point at an empty bundle-local namespace. Tracked seed
# histories remain authoritative, while untracked workstation state cannot
# change a clean release decision.
GENESIS_AGENT_GAUNTLET_PROFILE="$PROFILE" \
GENESIS_AGENT_GAUNTLET_REQUIRE_GPU_DEVICE_BACKEND=1 \
GENESIS_AGENT_GAUNTLET_REGRESSION_SLACK_MS=1500 \
bash scripts/render_agent_reference_workflows_report.sh \
  "$GAUNTLET_REPORT" \
  "$GAUNTLET_HISTORY" \
  "$INPUT_ROOT/agent_capability_gauntlet_history.jsonl"

GENESIS_RUNTIME_BACKEND_MATRIX_EPHEMERAL_TARGET_DIR="$OUTPUT_ROOT/runtime-backend-target" \
GENESIS_RUNTIME_BACKEND_MATRIX_CARGO_PROFILE_DEV_DEBUG=0 \
GENESIS_RUNTIME_BACKEND_MATRIX_CARGO_INCREMENTAL=0 \
bash scripts/render_runtime_backend_feature_matrix_report.sh \
  "$RUNTIME_BACKEND_REPORT" \
  "$RUNTIME_BACKEND_HISTORY" \
  "$INPUT_ROOT/runtime_backend_feature_matrix_history.jsonl"

GENESIS_HOST_BRIDGE_FAULT_RUNS="$([[ "$PROFILE" == "release-full" ]] && echo 6 || echo 1)" \
GENESIS_HOST_BRIDGE_FAULT_MAX_FAILURE_RATE_PCT=0 \
GENESIS_HOST_BRIDGE_FAULT_BUDGET_MS="$([[ "$PROFILE" == "release-full" ]] && echo 300000 || echo 120000)" \
bash scripts/render_host_bridge_fault_injection_report.sh \
  "$HOST_BRIDGE_REPORT" \
  "$HOST_BRIDGE_HISTORY" \
  "$INPUT_ROOT/host_bridge_fault_injection_history.jsonl"

bash scripts/render_webxr_browser_conformance_report.sh "$WEBXR_REPORT"
GENESIS_GPU_XR_REQUIRE_WEBXR_RUNTIME_EVIDENCE=1 \
bash scripts/render_gpu_xr_productization_kits_report.sh \
  "$GPU_XR_REPORT" \
  "$GAUNTLET_REPORT" \
  "$WEBXR_REPORT"

bash scripts/render_assurance_profile_packs_report.sh \
  "$ASSURANCE_REPORT" \
  "$ASSURANCE_HISTORY"

GENESIS_AGENT_PARITY_GAUNTLET_PROFILE=prepush-standard \
GENESIS_AGENT_PARITY_REUSE_REPORTS=0 \
GENESIS_AGENT_GAUNTLET_REGRESSION_SLACK_MS=1500 \
bash scripts/render_agent_workflow_runtime_parity_report.sh \
  "$PARITY_REPORT" \
  "$PARITY_HISTORY" \
  "$INPUT_ROOT/agent_workflow_runtime_parity_history.jsonl" \
  "$NATIVE_REPORT" \
  "$NATIVE_HISTORY" \
  "$INPUT_ROOT/agent_capability_gauntlet_native_report.json" \
  "$INPUT_ROOT/agent_capability_gauntlet_native_history.jsonl" \
  "$WASI_REPORT" \
  "$WASI_HISTORY" \
  "$INPUT_ROOT/agent_capability_gauntlet_wasi_report.json" \
  "$INPUT_ROOT/agent_capability_gauntlet_wasi_history.jsonl" \
  "$GENERATIVE_REPORT" \
  "$GENERATIVE_HISTORY" \
  "$INPUT_ROOT/agent_generative_workloads_history.jsonl"

python3 scripts/lib/health_profile_evidence.py build \
  --root "$ROOT_DIR" \
  --profile "$PROFILE" \
  --output-root "$OUTPUT_ROOT"
