#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

REPORT_PATH="${GENESIS_WRITE_SKILL_CONFORMANCE_REPORT:-.genesis/perf/write_genesiscode_skill_conformance_report.json}"
HISTORY_PATH="${GENESIS_WRITE_SKILL_CONFORMANCE_HISTORY:-.genesis/perf/write_genesiscode_skill_conformance_history.jsonl}"
MANIFEST_INPUT="${GENESIS_WRITE_SKILL_CONFORMANCE_MANIFEST:-docs/skill_pack/write_genesiscode_v1/manifest.json}"
GAUNTLET_INPUT="${GENESIS_WRITE_SKILL_GAUNTLET_REPORT:-.genesis/perf/agent_capability_gauntlet_report.json}"
GENERATIVE_INPUT="${GENESIS_WRITE_SKILL_GENERATIVE_REPORT:-.genesis/perf/agent_generative_workloads_report.json}"
RUNTIME_BACKEND_INPUT="${GENESIS_WRITE_SKILL_RUNTIME_BACKEND_REPORT:-.genesis/perf/runtime_backend_feature_matrix_report.json}"
HOST_BRIDGE_INPUT="${GENESIS_WRITE_SKILL_HOST_BRIDGE_REPORT:-.genesis/perf/host_bridge_fault_injection_report.json}"
GPU_XR_INPUT="${GENESIS_WRITE_SKILL_GPU_XR_REPORT:-.genesis/perf/gpu_xr_productization_kits_report.json}"
ASSURANCE_INPUT="${GENESIS_WRITE_SKILL_ASSURANCE_REPORT:-.genesis/perf/assurance_profile_packs_report.json}"

exec bash scripts/render_write_genesiscode_skill_conformance_report.sh \
  "$REPORT_PATH" \
  "$HISTORY_PATH" \
  "$HISTORY_PATH" \
  "$MANIFEST_INPUT" \
  "$GAUNTLET_INPUT" \
  "$GENERATIVE_INPUT" \
  "$RUNTIME_BACKEND_INPUT" \
  "$HOST_BRIDGE_INPUT" \
  "$GPU_XR_INPUT" \
  "$ASSURANCE_INPUT"
