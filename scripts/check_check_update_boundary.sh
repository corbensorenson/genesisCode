#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

snapshot_migrated_reports() {
  python3 - <<'PY'
from hashlib import sha256
from pathlib import Path
import json

paths = (
    Path('.genesis/perf/doc_complexity_report.json'),
    Path('.genesis/perf/cargo_target_dir_policy_report.json'),
    Path('.genesis/perf/cargo_target_dir_policy_history.jsonl'),
    Path('.genesis/perf/kernel_tcb_contract_report.json'),
    Path('.genesis/perf/host_api_evolution_contract_report.json'),
    Path('.genesis/perf/tool_qualification_lineage_report.json'),
    Path('.genesis/perf/selfhost_gc_migration_plan_report.json'),
    Path('.genesis/perf/source_decomposition_progress_report.json'),
    Path('.genesis/perf/assurance_profile_packs_report.json'),
    Path('.genesis/perf/assurance_profile_packs_history.jsonl'),
    Path('.genesis/perf/assurance_standards_crosswalk_report.json'),
    Path('.genesis/perf/assurance_standards_crosswalk_history.jsonl'),
    Path('.genesis/perf/upgrade_plan_health_profile_report.json'),
    Path('.genesis/perf/upgrade_plan_health_profile_history.jsonl'),
    Path('.genesis/perf/upgrade_plan_health_agent_inner_loop_report.json'),
    Path('.genesis/perf/upgrade_plan_health_agent_inner_loop_history.jsonl'),
    Path('.genesis/perf/upgrade_plan_health_prepush_history.jsonl'),
    Path('.genesis/perf/upgrade_plan_health_release_full_history.jsonl'),
    Path('.genesis/perf/upgrade_plan_health_warmup_dev-fast.json'),
    Path('.genesis/perf/upgrade_plan_health_disk_preflight_report.json'),
)
print(json.dumps({
    path.as_posix(): sha256(path.read_bytes()).hexdigest() if path.is_file() else None
    for path in paths
}, sort_keys=True))
PY
}

python3 scripts/lib/check_update_boundary.py --self-test
canary_dir="$(mktemp -d)"
trap 'rm -rf "$canary_dir"' EXIT
before="$(snapshot_migrated_reports)"
bash scripts/check_doc_complexity_budget.sh >/dev/null
bash scripts/check_cargo_target_dir_policy.sh >/dev/null
bash scripts/check_kernel_tcb_contract.sh >/dev/null
GENESIS_HOST_API_EVOLUTION_REPORT="$canary_dir/host-api.json" \
  bash scripts/check_host_api_evolution_contracts.sh >/dev/null
GENESIS_TOOL_QUALIFICATION_LINEAGE_REPORT="$canary_dir/tool-lineage.json" \
  bash scripts/check_tool_qualification_lineage.sh >/dev/null
GENESIS_SELFHOST_GC_MIGRATION_PLAN_REPORT="$canary_dir/migration-plan.json" \
  bash scripts/check_selfhost_gc_migration_plan.sh >/dev/null
GENESIS_SOURCE_DECOMPOSITION_REPORT="$canary_dir/source-decomposition.json" \
  bash scripts/check_source_decomposition_progress.sh >/dev/null
GENESIS_ASSURANCE_PROFILE_PACKS_REPORT="$canary_dir/assurance-packs.json" \
GENESIS_ASSURANCE_PROFILE_PACKS_HISTORY="$canary_dir/assurance-packs.jsonl" \
  bash scripts/check_assurance_profile_packs.sh >/dev/null
GENESIS_ASSURANCE_STANDARDS_CROSSWALK_REPORT="$canary_dir/assurance-crosswalk.json" \
GENESIS_ASSURANCE_STANDARDS_CROSSWALK_HISTORY="$canary_dir/assurance-crosswalk.jsonl" \
  bash scripts/check_assurance_standards_crosswalk.sh >/dev/null
env -u CARGO_TARGET_DIR \
  -u GENESIS_CARGO_CACHE_RESOLVED \
  -u GENESIS_CARGO_CACHE_SCOPE \
  -u GENESIS_CARGO_CACHE_KEY_SHA256 \
  -u GENESIS_CARGO_CACHE_HIT \
  -u GENESIS_CARGO_CACHE_RUSTC_IDENTITY_JSON \
GENESIS_HEALTH_TEST_GATE_OVERRIDE=true \
GENESIS_HEALTH_ENFORCE_GATES=1 \
GENESIS_CARGO_CACHE_ROOT="$canary_dir/cargo-cache" \
GENESIS_HEALTH_PROFILE_REPORT="$canary_dir/health-profile.json" \
GENESIS_HEALTH_PROFILE_HISTORY="$canary_dir/health-profile.jsonl" \
GENESIS_HEALTH_AGENT_INNER_LOOP_HISTORY="$canary_dir/health-agent.jsonl" \
GENESIS_HEALTH_PREPUSH_HISTORY="$canary_dir/health-prepush.jsonl" \
GENESIS_HEALTH_RELEASE_FULL_HISTORY="$canary_dir/health-release.jsonl" \
GENESIS_HEALTH_WARMUP_REPORT="$canary_dir/health-warmup.json" \
GENESIS_HEALTH_DISK_PREFLIGHT_REPORT="$canary_dir/health-disk.json" \
  bash scripts/check_upgrade_plan_health.sh --profile dev-fast >/dev/null
after="$(snapshot_migrated_reports)"
[[ "$before" == "$after" ]] || {
  echo "check-update-boundary: migrated checks mutated persistent reports" >&2
  exit 1
}
if find "$canary_dir" \
  -path "$canary_dir/cargo-cache" -prune -o \
  -type f -print -quit | grep -q .; then
  echo "check-update-boundary: migrated check honored a caller-controlled report path" >&2
  find "$canary_dir" \
    -path "$canary_dir/cargo-cache" -prune -o \
    -type f -print >&2
  exit 1
fi
for renderer in \
  scripts/render_host_api_evolution_contract_report.sh \
  scripts/render_tool_qualification_lineage_report.sh \
  scripts/render_selfhost_gc_migration_plan_report.sh \
  scripts/render_source_decomposition_progress_report.sh \
  scripts/render_assurance_profile_packs_report.sh \
  scripts/render_assurance_standards_crosswalk_report.sh \
  scripts/render_no_user_panics_report.sh \
  scripts/render_selfhost_artifact_fresh_report.sh \
  scripts/render_selfhost_dashboard_fresh_report.sh \
  scripts/render_selfhost_readiness_scorecard_report.sh \
  scripts/render_bootstrap_retirement_gate_report.sh \
  scripts/render_full_selfhost_cutover_profile_report.sh \
  scripts/render_remote_registry_runtime_parity_report.sh \
  scripts/render_gcpm_operation_contract_pack_report.sh \
  scripts/render_vcs_selfhost_contract_report.sh \
  scripts/render_selfhost_symbol_ownership_report.sh \
  scripts/render_cli_diagnostics_contract_report.sh \
  scripts/render_foundation_stdlib_conformance_report.sh \
  scripts/render_fuzz_differential_hardening_report.sh \
  scripts/render_wasm_production_surface_report.sh \
  scripts/render_webxr_browser_conformance_report.sh \
  scripts/render_gfx_runtime_profile_report.sh \
  scripts/render_production_cli_help_surface_report.sh \
  scripts/render_production_cli_parse_surface_report.sh \
  scripts/render_agent_reference_workflows_report.sh \
  scripts/render_agent_generative_workloads_report.sh \
  scripts/render_agent_scenario_perf_report.sh \
  scripts/render_agent_workflow_runtime_parity_report.sh \
  scripts/render_runtime_microbench_budgets_report.sh \
  scripts/render_gpu_compute_runtime_profile_report.sh \
  scripts/render_gpu_compute_device_conformance_report.sh \
  scripts/render_gpu_device_conformance_lane_parity_report.sh \
  scripts/render_gpu_device_conformance_matrix_report.sh \
  scripts/render_gpu_gfx_headroom_conformance_report.sh \
  scripts/render_gpu_xr_productization_kits_report.sh \
  scripts/render_task_concurrency_stress_report.sh \
  scripts/render_host_bridge_fault_injection_report.sh \
  scripts/render_hot_path_budgets_report.sh \
  scripts/render_perf_budgets_report.sh \
  scripts/render_runtime_workload_budgets_report.sh \
  scripts/render_ai_iteration_slo_report.sh \
  scripts/render_ai_stress_suite_report.sh \
  scripts/render_backend_starter_workflows_report.sh \
  scripts/render_domain_starter_registry_bootstrap_report.sh \
  scripts/render_full_cross_host_profile_budget_report.sh \
  scripts/render_gcpm_target_runtime_pipelines_report.sh \
  scripts/render_runtime_backend_feature_matrix_report.sh \
  scripts/render_write_genesiscode_skill_conformance_report.sh \
  scripts/render_source_decomposition_tracked_parity_report.sh \
  scripts/render_large_workspace_agent_perf_report.sh \
  scripts/render_upgrade_plan_health_report.sh
do
  if bash "$renderer" >/dev/null 2>&1; then
    echo "check-update-boundary: renderer accepted a missing explicit output path: $renderer" >&2
    exit 1
  fi
done
heavy_wrapper_contracts=0
while IFS='|' read -r check renderer report_var history_var; do
  [[ -n "$check" ]] || continue
  if ! grep -Fq "$renderer" "$check"; then
    echo "check-update-boundary: heavy check does not delegate to reviewed renderer: $check" >&2
    exit 1
  fi
  if grep -Fq "$report_var" "$check" || grep -Fq "$history_var" "$check"; then
    echo "check-update-boundary: heavy check still accepts persistent output overrides: $check" >&2
    exit 1
  fi
  heavy_wrapper_contracts=$((heavy_wrapper_contracts + 1))
done <<'EOF'
scripts/check_no_user_panics_compiler.sh|scripts/render_no_user_panics_report.sh|GENESIS_NO_USER_PANICS_REPORT|GENESIS_NO_USER_PANICS_HISTORY
scripts/check_selfhost_artifact_fresh.sh|scripts/render_selfhost_artifact_fresh_report.sh|GENESIS_SELFHOST_ARTIFACT_FRESH_REPORT|GENESIS_SELFHOST_ARTIFACT_FRESH_HISTORY
scripts/check_selfhost_dashboard_fresh.sh|scripts/render_selfhost_dashboard_fresh_report.sh|GENESIS_SELFHOST_DASHBOARD_FRESH_REPORT|GENESIS_SELFHOST_DASHBOARD_FRESH_HISTORY
scripts/check_selfhost_readiness_scorecard.sh|scripts/render_selfhost_readiness_scorecard_report.sh|PERSISTENT_REPORT_PATH=|PERSISTENT_HISTORY_PATH=
scripts/check_bootstrap_retirement_gate.sh|scripts/render_bootstrap_retirement_gate_report.sh|PERSISTENT_REPORT_PATH=|PERSISTENT_HISTORY_PATH=
scripts/check_full_selfhost_cutover_profile.sh|scripts/render_full_selfhost_cutover_profile_report.sh|PERSISTENT_REPORT_PATH=|PERSISTENT_HISTORY_PATH=
scripts/check_remote_registry_runtime_parity.sh|scripts/render_remote_registry_runtime_parity_report.sh|PERSISTENT_REPORT_PATH=|PERSISTENT_HISTORY_PATH=
scripts/check_gcpm_operation_contract_pack.sh|scripts/render_gcpm_operation_contract_pack_report.sh|PERSISTENT_REPORT_PATH=|PERSISTENT_HISTORY_PATH=
scripts/check_vcs_selfhost_contract.sh|scripts/render_vcs_selfhost_contract_report.sh|PERSISTENT_REPORT_PATH=|PERSISTENT_HISTORY_PATH=
scripts/check_selfhost_symbol_ownership.sh|scripts/render_selfhost_symbol_ownership_report.sh|PERSISTENT_REPORT_PATH=|PERSISTENT_HISTORY_PATH=
scripts/check_cli_diagnostics_contract.sh|scripts/render_cli_diagnostics_contract_report.sh|PERSISTENT_REPORT_PATH=|PERSISTENT_HISTORY_PATH=
scripts/check_foundation_stdlib_conformance.sh|scripts/render_foundation_stdlib_conformance_report.sh|PERSISTENT_REPORT_PATH=|PERSISTENT_HISTORY_PATH=
scripts/check_fuzz_differential_hardening.sh|scripts/render_fuzz_differential_hardening_report.sh|PERSISTENT_REPORT_PATH=|PERSISTENT_HISTORY_PATH=
scripts/check_wasm_production_surface.sh|scripts/render_wasm_production_surface_report.sh|PERSISTENT_REPORT_PATH=|PERSISTENT_HISTORY_PATH=
scripts/check_webxr_browser_conformance.sh|scripts/render_webxr_browser_conformance_report.sh|PERSISTENT_REPORT_PATH=|PERSISTENT_HISTORY_PATH=
scripts/check_gfx_runtime_profile.sh|scripts/render_gfx_runtime_profile_report.sh|PERSISTENT_REPORT_PATH=|PERSISTENT_HISTORY_PATH=
scripts/check_production_cli_help_surface.sh|scripts/render_production_cli_help_surface_report.sh|PERSISTENT_REPORT_PATH=|PERSISTENT_HISTORY_PATH=
scripts/check_production_cli_parse_surface.sh|scripts/render_production_cli_parse_surface_report.sh|PERSISTENT_REPORT_PATH=|PERSISTENT_HISTORY_PATH=
scripts/check_agent_reference_workflows.sh|scripts/render_agent_reference_workflows_report.sh|PERSISTENT_REPORT_PATH=|PERSISTENT_HISTORY_PATH=
scripts/check_agent_generative_workloads.sh|scripts/render_agent_generative_workloads_report.sh|PERSISTENT_REPORT_PATH=|PERSISTENT_HISTORY_PATH=
scripts/check_agent_scenario_perf.sh|scripts/render_agent_scenario_perf_report.sh|PERSISTENT_REPORT_PATH=|PERSISTENT_HISTORY_PATH=
scripts/check_agent_workflow_runtime_parity.sh|scripts/render_agent_workflow_runtime_parity_report.sh|PERSISTENT_REPORT_PATH=|PERSISTENT_HISTORY_PATH=
scripts/check_runtime_microbench_budgets.sh|scripts/render_runtime_microbench_budgets_report.sh|PERSISTENT_REPORT_PATH=|PERSISTENT_HISTORY_PATH=
scripts/check_gpu_compute_runtime_profile.sh|scripts/render_gpu_compute_runtime_profile_report.sh|PERSISTENT_REPORT_PATH=|PERSISTENT_HISTORY_PATH=
scripts/check_gpu_compute_device_conformance.sh|scripts/render_gpu_compute_device_conformance_report.sh|PERSISTENT_REPORT_PATH=|PERSISTENT_HISTORY_PATH=
scripts/check_gpu_device_conformance_lane_parity.sh|scripts/render_gpu_device_conformance_lane_parity_report.sh|GENESIS_GPU_DEVICE_PARITY_REPORT_OUT|PERSISTENT_REPORT_PATH=
scripts/check_gpu_device_conformance_matrix.sh|scripts/render_gpu_device_conformance_matrix_report.sh|GENESIS_GPU_DEVICE_MATRIX_REPORT_OUT|PERSISTENT_REPORT_PATH=
scripts/check_gpu_gfx_headroom_conformance.sh|scripts/render_gpu_gfx_headroom_conformance_report.sh|GENESIS_GPU_GFX_HEADROOM_REPORT_OUT|GENESIS_GPU_GFX_HEADROOM_HISTORY_OUT
scripts/check_gpu_xr_productization_kits.sh|scripts/render_gpu_xr_productization_kits_report.sh|GENESIS_GPU_XR_PRODUCTIZATION_REPORT|PERSISTENT_REPORT_PATH=
scripts/check_task_concurrency_stress.sh|scripts/render_task_concurrency_stress_report.sh|GENESIS_TASK_STRESS_REPORT|PERSISTENT_REPORT_PATH=
scripts/check_host_bridge_fault_injection.sh|scripts/render_host_bridge_fault_injection_report.sh|GENESIS_HOST_BRIDGE_FAULT_REPORT|PERSISTENT_REPORT_PATH=
scripts/check_hot_path_budgets.sh|scripts/render_hot_path_budgets_report.sh|GENESIS_HOT_PATH_METRICS_OUT|GENESIS_HOT_PATH_RUNTIME_HISTORY_OUT
scripts/check_perf_budgets.sh|scripts/render_perf_budgets_report.sh|GENESIS_PERF_BUDGET_REPORT_OUT|GENESIS_PERF_BUDGET_HISTORY_OUT
scripts/check_runtime_workload_budgets.sh|scripts/render_runtime_workload_budgets_report.sh|GENESIS_RUNTIME_WORKLOAD_OUT|GENESIS_RUNTIME_WORKLOAD_HISTORY
scripts/check_ai_iteration_slo.sh|scripts/render_ai_iteration_slo_report.sh|GENESIS_AI_ITERATION_SLO_OUT|GENESIS_AI_ITERATION_SLO_HISTORY
scripts/check_ai_stress_suite.sh|scripts/render_ai_stress_suite_report.sh|GENESIS_STRESS_REPORT|GENESIS_STRESS_HISTORY
scripts/check_backend_starter_workflows.sh|scripts/render_backend_starter_workflows_report.sh|GENESIS_BACKEND_STARTER_REPORT|GENESIS_BACKEND_STARTER_HISTORY
scripts/check_domain_starter_registry_bootstrap.sh|scripts/render_domain_starter_registry_bootstrap_report.sh|GENESIS_DOMAIN_STARTER_REGISTRY_BOOTSTRAP_REPORT|PERSISTENT_REPORT_PATH=
scripts/check_full_cross_host_profile_budget.sh|scripts/render_full_cross_host_profile_budget_report.sh|GENESIS_FULL_CROSS_HOST_PROFILE_REPORT|GENESIS_FULL_CROSS_HOST_PROFILE_HISTORY
scripts/check_gcpm_target_runtime_pipelines.sh|scripts/render_gcpm_target_runtime_pipelines_report.sh|GENESIS_GCPM_TARGET_RUNTIME_EVIDENCE_REPORT|GENESIS_GCPM_TARGET_RUNTIME_EVIDENCE_DIR
scripts/check_runtime_backend_feature_matrix.sh|scripts/render_runtime_backend_feature_matrix_report.sh|GENESIS_RUNTIME_BACKEND_MATRIX_REPORT_OUT|GENESIS_RUNTIME_BACKEND_MATRIX_HISTORY_OUT
scripts/check_write_genesiscode_skill_conformance.sh|scripts/render_write_genesiscode_skill_conformance_report.sh|GENESIS_WRITE_SKILL_CONFORMANCE_REPORT|GENESIS_WRITE_SKILL_CONFORMANCE_HISTORY
scripts/check_source_decomposition_tracked_parity.sh|scripts/render_source_decomposition_tracked_parity_report.sh|GENESIS_SOURCE_DECOMPOSITION_TRACKED_PARITY_REPORT|PERSISTENT_REPORT_PATH=
scripts/check_large_workspace_agent_perf.sh|scripts/render_large_workspace_agent_perf_report.sh|GENESIS_LARGE_WORKSPACE_REPORT_OUT|GENESIS_LARGE_WORKSPACE_RUNTIME_HISTORY
scripts/check_upgrade_plan_health.sh|scripts/render_upgrade_plan_health_report.sh|GENESIS_HEALTH_PROFILE_REPORT|GENESIS_HEALTH_PROFILE_HISTORY
EOF
if grep -Fq 'check_selfhost_readiness_scorecard.sh' scripts/render_selfhost_dashboard_fresh_report.sh; then
  echo "check-update-boundary: dashboard freshness renderer still mutates readiness evidence transitively" >&2
  exit 1
fi
reclaim_controls=0
set +e
disk_output="$(bash scripts/check_disk_headroom.sh --auto-reclaim 1 2>&1)"
disk_rc=$?
set -e
if [[ "$disk_rc" -ne 2 || "$disk_output" != *"read-only"* ]]; then
  echo "check-update-boundary: disk reclaim negative control did not fail closed" >&2
  echo "$disk_output" >&2
  exit 1
fi
reclaim_controls=$((reclaim_controls + 1))
echo "check-update-boundary-read-only-smoke: ok (migrated_checks=51 render_contracts=51 heavy_wrapper_contracts=$heavy_wrapper_contracts reclaim_negative_controls=$reclaim_controls)"
exec python3 scripts/lib/check_update_boundary.py --check
