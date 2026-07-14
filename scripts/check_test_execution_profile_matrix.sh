#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

DOC="docs/spec/TEST_EXECUTION_PROFILES_v0.1.md"
GETTING_STARTED="docs/GETTING_STARTED.md"
CI=".github/workflows/ci.yml"
CHANGED_FAST_SCRIPT="scripts/test_changed_fast.sh"
UPDATE_CHANGED_FAST_SCRIPT="scripts/update_test_changed_fast_metrics.sh"
DOCS_QUICKSTART_SCRIPT="scripts/check_docs_quickstart.sh"
GREEN_FRONT_DOOR_SCRIPT="scripts/check_green_front_door.sh"
ROOT_LOCK_POLICY_SCRIPT="scripts/check_root_lock_policy.sh"
GENERATED_ARTIFACT_POLICY_SCRIPT="scripts/check_generated_artifact_policy.sh"
GATE_MANIFEST_SCRIPT="scripts/check_gate_manifest.sh"
GATE_MANIFEST_FILE="genesis.gates.json"
GATE_MANIFEST_SCHEMA="docs/spec/GATE_MANIFEST_v0.1.schema.json"
GATE_MANIFEST_POLICY="policies/gates_v0.1.json"
GENESIS_EVIDENCE_PROFILE_SCRIPT="scripts/check_genesis_evidence_profile.sh"
GENESIS_EVIDENCE_VERIFIER_SCRIPT="scripts/check_genesis_evidence_verifier.sh"
EVIDENCE_STORAGE_CLASSES_SCRIPT="scripts/check_evidence_storage_classes.sh"
VERSIONING_RELEASE_HYGIENE_SCRIPT="scripts/check_versioning_release_hygiene.sh"
SUPPLY_CHAIN_SCRIPT="scripts/check_supply_chain.sh"
RELEASE_SMOKE_SCRIPT="scripts/check_release_smoke.sh"
RELEASE_NOTES_SCRIPT="scripts/check_release_notes.sh"
RELEASE_NOTES_UPDATE="scripts/update_release_notes.sh"
GC_AGENT_PROFILE_SCRIPT="scripts/check_gc_agent_profile.sh"
GC_AGENT_PROFILE_UPDATE="scripts/update_gc_agent_profile.sh"
GC_AGENT_PROFILE="docs/spec/GC_AGENT_PROFILE_v0.3.json"
GC_AGENT_CORE_CARD_SCRIPT="scripts/check_gc_agent_core_card.sh"
GC_AGENT_CORE_CARD_UPDATE="scripts/update_gc_agent_core_card.sh"
GC_AGENT_CORE_CARD="docs/spec/GC_AGENT_CORE_CARD_v0.3.md"
GC_AGENT_CORE_CARD_MANIFEST="docs/spec/GC_AGENT_CORE_CARD_v0.3.json"
GC_AGENT_TASK_CARDS_SCRIPT="scripts/check_gc_agent_task_cards.sh"
GC_AGENT_TASK_CARDS_UPDATE="scripts/update_gc_agent_task_cards.sh"
GC_AGENT_TASK_CARDS="docs/spec/GC_AGENT_TASK_CARDS_v0.3.md"
GC_AGENT_TASK_CARDS_REGISTRY="docs/spec/GC_AGENT_TASK_CARDS_v0.3.json"
GC_AGENT_SYMBOL_INDEX_SCRIPT="scripts/check_gc_agent_symbol_index.sh"
GC_AGENT_SYMBOL_INDEX_UPDATE="scripts/update_gc_agent_symbol_index.sh"
GC_AGENT_SYMBOL_INDEX="docs/spec/GC_AGENT_SYMBOL_INDEX_v0.3.json"
GC_AGENT_SYMBOL_INDEX_SCHEMA="docs/spec/GC_AGENT_SYMBOL_INDEX_v0.3.schema.json"
PERF_GATES_SCRIPT="scripts/test_perf_gates.sh"
DEFAULT_LOOP_SCRIPT="scripts/check_default_iteration_workflow.sh"
STRICT_GOLDEN_SCRIPT="scripts/selfhost_strict_golden.sh"
WASM_CROSS_HOST_SCRIPT="scripts/wasm_cross_host_determinism.mjs"
FULL_CROSS_HOST_BUDGET_SCRIPT="scripts/check_full_cross_host_profile_budget.sh"
FULL_CROSS_HOST_RENDERER="scripts/render_full_cross_host_profile_budget_report.sh"
FULL_CROSS_HOST_UPDATE_SCRIPT="scripts/update_full_cross_host_profile_budget_report.sh"
RUNTIME_WORKLOAD_SCRIPT="scripts/check_runtime_workload_budgets.sh"
RUNTIME_WORKLOAD_SEED_HISTORY="policies/perf/runtime_workload_bench_runtime_seed_history.jsonl"
ROADMAP_WORKLOAD_SCRIPT="scripts/check_roadmap_workloads.sh"
ROADMAP_WORKLOAD_POLICY="policies/perf/roadmap_workloads_v0.1.json"
ROADMAP_BASELINE_SCRIPT="scripts/check_roadmap_baseline.sh"
ROADMAP_BASELINE_UPDATE="scripts/update_roadmap_baseline.sh"
LARGE_WORKSPACE_SCRIPT="scripts/check_large_workspace_agent_perf.sh"
LARGE_WORKSPACE_UPDATE_SCRIPT="scripts/update_large_workspace_agent_perf_report.sh"
SOURCE_PARITY_SCRIPT="scripts/check_source_decomposition_tracked_parity.sh"
SOURCE_PARITY_UPDATE_SCRIPT="scripts/update_source_decomposition_tracked_parity_report.sh"
HEALTH_RENDERER="scripts/render_upgrade_plan_health_report.sh"
HEALTH_UPDATE_SCRIPT="scripts/update_upgrade_plan_health_report.sh"
ROADMAP_EXECUTION_CHECK="scripts/check_roadmap_execution_manifest.sh"
ROADMAP_EXECUTION_UPDATE="scripts/update_roadmap_execution_manifest.sh"
ROADMAP_EXECUTION_SCHEMA="docs/spec/ROADMAP_EXECUTION_MANIFEST_v0.1.schema.json"
ROADMAP_EXECUTION_MANIFEST="docs/program/ROADMAP_EXECUTION_MANIFEST_v0.1.json"
AGENT_GENERATIVE_CHECK="scripts/check_agent_generative_workloads.sh"
AGENT_GENERATIVE_RENDERER="scripts/render_agent_generative_workloads_report.sh"
AGENT_SCENARIO_CHECK="scripts/check_agent_scenario_perf.sh"
AGENT_SCENARIO_RENDERER="scripts/render_agent_scenario_perf_report.sh"
CARGO_TARGET_POLICY_SCRIPT="scripts/check_cargo_target_dir_policy.sh"
GATE_TELEMETRY_SCRIPT="scripts/check_gate_resource_telemetry.sh"
GATE_TELEMETRY_RUNNER="scripts/lib/gate_telemetry.py"
GATE_TELEMETRY_SCHEMA="docs/spec/GATE_RESOURCE_TELEMETRY_v0.1.schema.json"
GATE_TELEMETRY_POLICY="policies/gate_telemetry_v0.1.json"
DETERMINISTIC_CLEANUP_SCRIPT="scripts/check_deterministic_cleanup.sh"
DETERMINISTIC_CLEANUP_RUNNER="scripts/lib/deterministic_cleanup.py"
DETERMINISTIC_CLEANUP_POLICY="policies/deterministic_cleanup_v0.1.json"
DETERMINISTIC_CLEANUP_POLICY_SCHEMA="docs/spec/DETERMINISTIC_CLEANUP_POLICY_v0.1.schema.json"
DETERMINISTIC_CLEANUP_MARKER_SCHEMA="docs/spec/DETERMINISTIC_CLEANUP_MARKER_v0.1.schema.json"
DETERMINISTIC_CLEANUP_PLAN_SCHEMA="docs/spec/DETERMINISTIC_CLEANUP_PLAN_v0.1.schema.json"
DETERMINISTIC_CLEANUP_RESULT_SCHEMA="docs/spec/DETERMINISTIC_CLEANUP_RESULT_v0.1.schema.json"
UPGRADE_PLAN_SYNC_SCRIPT="scripts/sync_upgrade_plan_state.sh"
AGENT_GPU_PROFILE_CONTRACT_SCRIPT="scripts/check_agent_gpu_profile_contract.sh"
AGENT_GPU_PROFILE_LIB="scripts/lib/agent_gpu_profile_contract.sh"
DENY_CONFIG="deny.toml"

for path in \
  "$DOC" \
  "$GETTING_STARTED" \
  "$CI" \
  "$CHANGED_FAST_SCRIPT" \
  "$UPDATE_CHANGED_FAST_SCRIPT" \
  "$DOCS_QUICKSTART_SCRIPT" \
  "$GREEN_FRONT_DOOR_SCRIPT" \
  "$ROOT_LOCK_POLICY_SCRIPT" \
  "$GENERATED_ARTIFACT_POLICY_SCRIPT" \
  "$GATE_MANIFEST_SCRIPT" \
  "$GATE_MANIFEST_FILE" \
  "$GATE_MANIFEST_SCHEMA" \
  "$GATE_MANIFEST_POLICY" \
  "$GENESIS_EVIDENCE_PROFILE_SCRIPT" \
  "$GENESIS_EVIDENCE_VERIFIER_SCRIPT" \
  "$EVIDENCE_STORAGE_CLASSES_SCRIPT" \
  "$VERSIONING_RELEASE_HYGIENE_SCRIPT" \
  "$SUPPLY_CHAIN_SCRIPT" \
  "$RELEASE_SMOKE_SCRIPT" \
  "$RELEASE_NOTES_SCRIPT" \
  "$RELEASE_NOTES_UPDATE" \
  "$GC_AGENT_PROFILE_SCRIPT" \
  "$GC_AGENT_PROFILE_UPDATE" \
  "$GC_AGENT_PROFILE" \
  "$GC_AGENT_CORE_CARD_SCRIPT" \
  "$GC_AGENT_CORE_CARD_UPDATE" \
  "$GC_AGENT_CORE_CARD" \
  "$GC_AGENT_CORE_CARD_MANIFEST" \
  "$GC_AGENT_TASK_CARDS_SCRIPT" \
  "$GC_AGENT_TASK_CARDS_UPDATE" \
  "$GC_AGENT_TASK_CARDS" \
  "$GC_AGENT_TASK_CARDS_REGISTRY" \
  "$PERF_GATES_SCRIPT" \
  "$DEFAULT_LOOP_SCRIPT" \
  "$STRICT_GOLDEN_SCRIPT" \
  "$WASM_CROSS_HOST_SCRIPT" \
  "$FULL_CROSS_HOST_BUDGET_SCRIPT" \
  "$FULL_CROSS_HOST_RENDERER" \
  "$FULL_CROSS_HOST_UPDATE_SCRIPT" \
  "$RUNTIME_WORKLOAD_SCRIPT" \
  "$RUNTIME_WORKLOAD_SEED_HISTORY" \
  "$ROADMAP_WORKLOAD_SCRIPT" \
  "$ROADMAP_WORKLOAD_POLICY" \
  "$ROADMAP_BASELINE_SCRIPT" \
  "$ROADMAP_BASELINE_UPDATE" \
  "$LARGE_WORKSPACE_SCRIPT" \
  "$LARGE_WORKSPACE_UPDATE_SCRIPT" \
  "$SOURCE_PARITY_SCRIPT" \
  "$SOURCE_PARITY_UPDATE_SCRIPT" \
  "$HEALTH_RENDERER" \
  "$HEALTH_UPDATE_SCRIPT" \
  "$ROADMAP_EXECUTION_CHECK" \
  "$ROADMAP_EXECUTION_UPDATE" \
  "$ROADMAP_EXECUTION_SCHEMA" \
  "$ROADMAP_EXECUTION_MANIFEST" \
  "$AGENT_GENERATIVE_CHECK" \
  "$AGENT_GENERATIVE_RENDERER" \
  "$AGENT_SCENARIO_CHECK" \
  "$AGENT_SCENARIO_RENDERER" \
  "$CARGO_TARGET_POLICY_SCRIPT" \
  "$GATE_TELEMETRY_SCRIPT" \
  "$GATE_TELEMETRY_RUNNER" \
  "$GATE_TELEMETRY_SCHEMA" \
  "$GATE_TELEMETRY_POLICY" \
  "$DETERMINISTIC_CLEANUP_SCRIPT" \
  "$DETERMINISTIC_CLEANUP_RUNNER" \
  "$DETERMINISTIC_CLEANUP_POLICY" \
  "$DETERMINISTIC_CLEANUP_POLICY_SCHEMA" \
  "$DETERMINISTIC_CLEANUP_MARKER_SCHEMA" \
  "$DETERMINISTIC_CLEANUP_PLAN_SCHEMA" \
  "$DETERMINISTIC_CLEANUP_RESULT_SCHEMA" \
  "$UPGRADE_PLAN_SYNC_SCRIPT" \
  "$AGENT_GPU_PROFILE_CONTRACT_SCRIPT" \
  "$AGENT_GPU_PROFILE_LIB" \
  "$DENY_CONFIG"; do
  [[ -f "$path" ]] || {
    echo "test-execution-profile-matrix: missing required file: $path" >&2
    exit 1
  }
done

require_doc_pattern() {
  local pattern="$1"
  if ! grep -Fq "$pattern" "$DOC"; then
    echo "test-execution-profile-matrix: missing profile matrix entry in $DOC: $pattern" >&2
    exit 1
  fi
}

require_ci_pattern() {
  local pattern="$1"
  if ! grep -Fq "$pattern" "$CI"; then
    echo "test-execution-profile-matrix: missing CI profile step in $CI: $pattern" >&2
    exit 1
  fi
}

require_doc_pattern '| `smoke` |'
require_doc_pattern '| `changed-fast` |'
require_doc_pattern '| `perf-gate-regressions` |'
require_doc_pattern '| `agent-inner-loop` |'
require_doc_pattern '| `release-full` |'
require_doc_pattern '| `strict-golden` |'
require_doc_pattern '| `full-cross-host` |'
require_doc_pattern '`<= 2m`'
require_doc_pattern '`<= 5m`'
require_doc_pattern '`<= 30m`'
require_doc_pattern '`<= 3m`'
require_doc_pattern '`<= 8m`'
require_doc_pattern '`<= 12m`'
require_doc_pattern 'Preferred runner: `cargo nextest`'
require_doc_pattern 'Default `cargo test --workspace` contract'
require_doc_pattern '#[ignore = "perf-gate"]'
require_doc_pattern 'scripts/test_perf_gates.sh'
require_doc_pattern 'scripts/check_upgrade_plan_health.sh --profile agent-inner-loop'
require_doc_pattern 'scripts/check_upgrade_plan_health.sh --profile prepush-standard'
require_doc_pattern 'genesis/upgrade-plan-health-profile-v0.1'
require_doc_pattern '.genesis/perf/upgrade_plan_health_agent_inner_loop_report.json'
require_doc_pattern 'policies/perf/upgrade_plan_health_agent_inner_loop_seed_history.jsonl'
require_doc_pattern 'GENESIS_HEALTH_AGENT_INNER_LOOP_BUDGET_MS'
require_doc_pattern 'GENESIS_HEALTH_AGENT_INNER_LOOP_MIN_HISTORY'
require_doc_pattern 'GENESIS_HEALTH_AGENT_INNER_LOOP_REQUIRE_MIN_HISTORY'
require_doc_pattern 'GENESIS_HEALTH_AGENT_INNER_LOOP_BASELINE_HISTORY'
require_doc_pattern 'GENESIS_HEALTH_PREPUSH_BUDGET_MS'
require_doc_pattern 'GENESIS_HEALTH_PREPUSH_HISTORY'
require_doc_pattern 'GENESIS_HEALTH_PREPUSH_MIN_HISTORY'
require_doc_pattern 'GENESIS_HEALTH_PREPUSH_REQUIRE_MIN_HISTORY'
require_doc_pattern 'GENESIS_HEALTH_PREPUSH_BASELINE_HISTORY'
require_doc_pattern 'GENESIS_HEALTH_PREPUSH_HISTORY_SCOPE_KEY'
require_doc_pattern 'GENESIS_HEALTH_RELEASE_FULL_BUDGET_MS'
require_doc_pattern 'GENESIS_HEALTH_RELEASE_FULL_HISTORY'
require_doc_pattern 'GENESIS_HEALTH_RELEASE_FULL_MIN_HISTORY'
require_doc_pattern 'GENESIS_HEALTH_RELEASE_FULL_REQUIRE_MIN_HISTORY'
require_doc_pattern 'GENESIS_HEALTH_RELEASE_FULL_BASELINE_HISTORY'
require_doc_pattern 'GENESIS_HEALTH_RELEASE_FULL_HISTORY_SCOPE_KEY'
require_doc_pattern 'GENESIS_HEALTH_SHARDS'
require_doc_pattern 'content-addressed `root-host` cache'
require_doc_pattern 'GENESIS_HEALTH_CARGO_GATE_SHARDS'
require_doc_pattern 'GENESIS_HEALTH_WARM_CARGO_CACHE=auto|1|0'
require_doc_pattern 'GENESIS_HEALTH_PROFILE_GATE_CACHE=auto|1'
require_doc_pattern 'GENESIS_HEALTH_PROFILE_GATE_CACHE_TTL_SEC'
require_doc_pattern 'scripts/lib/run_cached_health_gate.sh'
require_doc_pattern 'genesis/upgrade-plan-health-cargo-warmup-v0.1'
require_doc_pattern 'release-full` renders current real-device and deterministic-device conformance'
require_doc_pattern 'AI Iteration SLO Contention Policy'
require_doc_pattern 'median-of-samples'
require_doc_pattern 'GENESIS_AI_ITERATION_SLO_SAMPLES_INCREMENTAL_WARM'
require_doc_pattern 'GENESIS_AI_ITERATION_SLO_CONTENTION_WARN_PERCENT'
require_doc_pattern 'GENESIS_AI_ITERATION_SLO_WARMUP_GCPM_LOCK'
require_doc_pattern 'GENESIS_AI_ITERATION_SLO_STABILIZE_RETRIES_GCPM_LOCK'
require_doc_pattern '.genesis/perf/strict_golden_profile_report.json'
require_doc_pattern '.genesis/perf/wasm_cross_host_profile_report.json'
require_doc_pattern '.genesis/perf/full_cross_host_profile_report.json'
require_doc_pattern '.genesis/perf/runtime_workload_bench_report.json'
require_doc_pattern '.genesis/perf/runtime_workload_bench_history.jsonl'
require_doc_pattern '.genesis/perf/runtime_workload_bench_runtime_report.json'
require_doc_pattern '.genesis/perf/runtime_workload_bench_runtime_history.jsonl'
require_doc_pattern '.genesis/perf/agent_scenario_perf_report.json'
require_doc_pattern '.genesis/perf/agent_generative_workloads_report.json'
require_doc_pattern '.genesis/perf/large_workspace_agent_perf_report.json'
require_doc_pattern '.genesis/perf/large_workspace_agent_runtime_report.json'
require_doc_pattern 'scripts/check_large_workspace_agent_perf.sh'
require_doc_pattern 'scripts/update_large_workspace_agent_perf_report.sh'
require_doc_pattern 'scripts/check_source_decomposition_tracked_parity.sh'
require_doc_pattern 'scripts/update_source_decomposition_tracked_parity_report.sh'
require_doc_pattern 'scripts/check_roadmap_execution_manifest.sh'
require_doc_pattern 'scripts/update_roadmap_execution_manifest.sh'
require_doc_pattern 'scripts/check_genesis_evidence_profile.sh'
require_doc_pattern 'scripts/update_genesis_evidence_profile.sh'
require_doc_pattern 'scripts/check_genesis_evidence_verifier.sh'
require_doc_pattern 'scripts/update_genesis_evidence_verifier_vectors.sh'
require_doc_pattern 'scripts/check_evidence_storage_classes.sh'
require_doc_pattern 'scripts/update_evidence_fixture_classification.sh'
require_doc_pattern 'scripts/update_evidence_release_asset.sh'
require_doc_pattern 'policies/perf/full_cross_host_profile_seed_history.jsonl'
require_doc_pattern 'policies/perf/runtime_workload_bench_runtime_seed_history.jsonl'
require_doc_pattern 'policies/perf/agent_scenario_perf_seed_history.jsonl'
require_doc_pattern 'scripts/check_full_cross_host_profile_budget.sh'
require_doc_pattern 'scripts/update_full_cross_host_profile_budget_report.sh'
require_doc_pattern 'scripts/check_runtime_workload_budgets.sh'
require_doc_pattern 'GENESIS_RUNTIME_WORKLOAD_PROFILE=roadmap'
require_doc_pattern 'GENESIS_RUNTIME_WORKLOAD_REQUIRE_ROADMAP_SIZES=1'
require_doc_pattern 'policies/perf/roadmap_workloads_v0.1.json'
require_doc_pattern 'scalar `best_of` reports are E0 diagnostics'
require_doc_pattern 'scripts/check_roadmap_baseline.sh'
require_doc_pattern 'scripts/update_roadmap_baseline.sh'
if ! grep -Fq 'benchmarks/**' "$GATE_MANIFEST_POLICY" || \
   ! grep -Fq 'benchmarks|prelude|selfhost|examples|tests' scripts/lib/gate_manifest.py; then
  echo "test-execution-profile-matrix: gate manifest must bind benchmark and Prelude fixture inputs" >&2
  exit 1
fi
require_doc_pattern 'scripts/check_agent_scenario_perf.sh'
require_doc_pattern 'scripts/check_agent_generative_workloads.sh'
require_doc_pattern 'scripts/check_cargo_target_dir_policy.sh'
require_doc_pattern 'scripts/check_changed_impact.sh'
require_doc_pattern 'scripts/check_wasm_production_surface.sh'
require_doc_pattern 'GENESIS_AGENT_GPU_PROFILE=agent-gpu-strict|agent-gpu-fallback'
require_doc_pattern 'scripts/check_agent_gpu_profile_contract.sh'
require_doc_pattern 'scripts/check_capability_indices.sh'
require_doc_pattern 'scripts/check_generated_artifact_policy.sh'
require_doc_pattern 'scripts/check_versioning_release_hygiene.sh'
require_doc_pattern 'scripts/check_supply_chain.sh'
require_doc_pattern 'scripts/check_release_smoke.sh'
require_doc_pattern 'scripts/check_release_notes.sh'
require_doc_pattern 'scripts/update_release_notes.sh'
require_doc_pattern 'docs/program/RELEASE_NOTES_v0.2.0.json'
require_doc_pattern 'docs/spec/GC_AGENT_PROFILE_v0.3.json'
require_doc_pattern 'scripts/check_gc_agent_profile.sh'
require_doc_pattern 'scripts/update_gc_agent_profile.sh'
require_doc_pattern 'docs/spec/GC_AGENT_CORE_CARD_v0.3.md'
require_doc_pattern 'docs/spec/GC_AGENT_CORE_CARD_v0.3.json'
require_doc_pattern 'scripts/check_gc_agent_core_card.sh'
require_doc_pattern 'scripts/update_gc_agent_core_card.sh'
require_doc_pattern 'docs/spec/GC_AGENT_TASK_CARDS_v0.3.md'
require_doc_pattern 'docs/spec/GC_AGENT_TASK_CARDS_v0.3.json'
require_doc_pattern 'scripts/check_gc_agent_task_cards.sh'
require_doc_pattern 'scripts/update_gc_agent_task_cards.sh'
require_doc_pattern 'docs/spec/GC_AGENT_SYMBOL_INDEX_v0.3.json'
require_doc_pattern 'docs/spec/GC_AGENT_SYMBOL_INDEX_v0.3.schema.json'
require_doc_pattern 'scripts/check_gc_agent_symbol_index.sh'
require_doc_pattern 'scripts/update_gc_agent_symbol_index.sh'
require_doc_pattern 'cargo-deny'
require_doc_pattern 'deny.toml'
require_doc_pattern 'CHANGELOG.md'
require_doc_pattern 'docs/spec/VERSIONING_v0.1.md'
require_doc_pattern 'docs/spec/RELEASE_SMOKE_v0.1.md'
require_doc_pattern 'scripts/update_test_changed_fast_metrics.sh'

require_ci_pattern 'Changed-File Fast Loop Budget'
require_ci_pattern 'Docs Quickstart Gate'
require_ci_pattern 'bash scripts/check_docs_quickstart.sh'
require_ci_pattern 'Ignored Perf Gate Regression Tests'
require_ci_pattern 'bash scripts/test_perf_gates.sh'
require_ci_pattern 'Local Workspace Test Contract (CI unset)'
require_ci_pattern 'env -u CI cargo test --workspace --profile selfhost-strict'
require_ci_pattern 'Selfhost Refactor Guard'
require_ci_pattern 'Selfhost Strict Smoke (Native + WASI CLI)'
require_ci_pattern 'Selfhost Strict Golden (Native + WASI CLI)'
require_ci_pattern 'WASM Cross-Host Determinism (Native vs Node)'
require_ci_pattern 'Full Cross-Host Runtime Budget Gate'
require_ci_pattern 'Full Cross-Host Runtime Budget Gate (PR Required)'
require_ci_pattern 'bash scripts/update_full_cross_host_profile_budget_report.sh'
require_ci_pattern 'Runtime Workload Budgets'
require_ci_pattern 'bash scripts/update_runtime_workload_budgets_report.sh'
require_ci_pattern 'AI Iteration SLO'
require_ci_pattern 'bash scripts/update_ai_iteration_slo_report.sh'
require_ci_pattern 'AI Stress Suite (Tasks + Bridge + GPU/Compute)'
require_ci_pattern 'bash scripts/update_ai_stress_suite_report.sh'
require_ci_pattern 'Backend Starter Workflow Evidence'
require_ci_pattern 'bash scripts/update_backend_starter_workflows_report.sh'
require_ci_pattern 'Domain Starter Registry Bootstrap Evidence'
require_ci_pattern 'bash scripts/update_domain_starter_registry_bootstrap_report.sh'
require_ci_pattern 'Agent End-to-End Scenario Perf Gate'
require_ci_pattern 'Agent Generative Workload Gate'
require_ci_pattern 'Capability Indices Guard'
require_ci_pattern 'bash scripts/check_capability_indices.sh'
require_ci_pattern 'Install cargo-deny'
require_ci_pattern 'Generated Artifact Policy Guard'
require_ci_pattern 'bash scripts/check_generated_artifact_policy.sh'
require_ci_pattern 'Genesis Evidence Profile Guard'
require_ci_pattern 'bash scripts/check_genesis_evidence_profile.sh'
require_ci_pattern 'Genesis Evidence Verifier Guard'
require_ci_pattern 'bash scripts/check_genesis_evidence_verifier.sh'
require_ci_pattern 'Evidence Storage Classes Guard'
require_ci_pattern 'bash scripts/check_evidence_storage_classes.sh'
require_ci_pattern 'Check Update Boundary Guard'
require_ci_pattern 'bash scripts/check_check_update_boundary.sh'
require_ci_pattern 'Gate Resource Telemetry Guard'
require_ci_pattern 'bash scripts/check_gate_resource_telemetry.sh'
require_ci_pattern 'Deterministic Cleanup Guard'
require_ci_pattern 'bash scripts/check_deterministic_cleanup.sh'
require_ci_pattern 'Gate Manifest Guard'
require_ci_pattern 'bash scripts/check_gate_manifest.sh'
require_ci_pattern 'Engineering Gate Budget Guard'
require_ci_pattern 'bash scripts/check_engineering_gate_contract.sh'
require_ci_pattern 'Reference Host Profile Guard'
require_ci_pattern 'bash scripts/check_reference_host_profiles.sh'
require_ci_pattern 'Roadmap Workload Normalization Guard'
require_ci_pattern 'bash scripts/check_roadmap_workloads.sh'
require_ci_pattern 'Signed Roadmap Baseline Guard'
require_ci_pattern 'bash scripts/check_roadmap_baseline.sh'
require_ci_pattern 'User-Path Panic Compiler Assurance'
require_ci_pattern 'bash scripts/check_no_user_panics_compiler.sh'
require_ci_pattern 'Versioning Release Hygiene Guard'
require_ci_pattern 'bash scripts/check_versioning_release_hygiene.sh'
require_ci_pattern 'Supply Chain Guard'
require_ci_pattern 'bash scripts/check_supply_chain.sh'
require_ci_pattern 'Release Smoke Gate'
require_ci_pattern 'bash scripts/check_release_smoke.sh'
require_ci_pattern 'Generated Release Notes Guard'
require_ci_pattern 'bash scripts/check_release_notes.sh'
require_ci_pattern 'GC Agent Profile Guard'
require_ci_pattern 'bash scripts/check_gc_agent_profile.sh'
require_ci_pattern 'GC Agent Core Card Guard'
require_ci_pattern 'bash scripts/check_gc_agent_core_card.sh'
require_ci_pattern 'GC Agent Task Cards Guard'
require_ci_pattern 'bash scripts/check_gc_agent_task_cards.sh'
require_ci_pattern 'GC Agent Symbol Index Guard'
require_ci_pattern 'bash scripts/check_gc_agent_symbol_index.sh'

if ! grep -Fq 'cargo nextest' "$GETTING_STARTED"; then
  echo "test-execution-profile-matrix: docs/GETTING_STARTED.md must document cargo-nextest as the preferred long-session runner" >&2
  exit 1
fi

if ! grep -Fq 'bash scripts/check_docs_quickstart.sh' "$GREEN_FRONT_DOOR_SCRIPT"; then
  echo "test-execution-profile-matrix: green-front-door must include docs quickstart gate" >&2
  exit 1
fi

if ! grep -Fq 'bash scripts/check_root_lock_policy.sh' "$GREEN_FRONT_DOOR_SCRIPT" || \
   ! grep -Fq 'genesisCode' "$ROOT_LOCK_POLICY_SCRIPT" || \
   ! grep -Fq 'genesis.lock' "$ROOT_LOCK_POLICY_SCRIPT"; then
  echo "test-execution-profile-matrix: green-front-door must include root genesis.lock policy conformance" >&2
  exit 1
fi
if grep -Eq 'python3|tomllib|tomli' "$ROOT_LOCK_POLICY_SCRIPT" || \
   ! grep -Fq 'parser=posix-awk' "$ROOT_LOCK_POLICY_SCRIPT" || \
   ! grep -Fq 'negative_controls' "$ROOT_LOCK_POLICY_SCRIPT"; then
  echo "test-execution-profile-matrix: root-lock check must use the dependency-free POSIX parser with adversarial controls" >&2
  exit 1
fi

for gate in \
  'bash scripts/check_capability_indices.sh' \
  'bash scripts/check_check_update_boundary.sh' \
  'bash scripts/check_deterministic_cleanup.sh' \
  'bash scripts/check_gate_resource_telemetry.sh' \
  'bash scripts/check_gate_manifest.sh' \
  'bash scripts/check_reference_host_profiles.sh' \
  'bash scripts/check_roadmap_workloads.sh' \
  'bash scripts/check_roadmap_baseline.sh' \
  'bash scripts/check_genesis_evidence_profile.sh' \
  'bash scripts/check_genesis_evidence_verifier.sh' \
  'bash scripts/check_evidence_storage_classes.sh' \
  'bash scripts/check_generated_artifact_policy.sh' \
  'bash scripts/check_gc_agent_profile.sh' \
  'bash scripts/check_gc_agent_core_card.sh' \
  'bash scripts/check_gc_agent_task_cards.sh' \
  'bash scripts/check_gc_agent_symbol_index.sh' \
  'bash scripts/check_release_notes.sh' \
  'bash scripts/check_versioning_release_hygiene.sh' \
  'bash scripts/check_supply_chain.sh' \
  'bash scripts/check_release_smoke.sh'; do
  if ! grep -Fq "$gate" "$GREEN_FRONT_DOOR_SCRIPT"; then
    echo "test-execution-profile-matrix: green-front-door missing release-hardening gate: $gate" >&2
    exit 1
  fi
done

if ! grep -Fq 'README.md" "docs/GETTING_STARTED.md' "$DOCS_QUICKSTART_SCRIPT"; then
  echo "test-execution-profile-matrix: docs quickstart gate must cover README.md and docs/GETTING_STARTED.md by default" >&2
  exit 1
fi

if ! grep -Fq 'cargo test -p gc_cli --test "$test_name"' "$PERF_GATES_SCRIPT" || \
   ! grep -Fq -- '-- --ignored --test-threads=1' "$PERF_GATES_SCRIPT" || \
   ! grep -Fq 'root-host' "$PERF_GATES_SCRIPT"; then
  echo "test-execution-profile-matrix: perf-gate runner must execute ignored gc_cli gate tests serially in the root-host cache" >&2
  exit 1
fi

for perf_gate_test in \
  crates/gc_cli/tests/upgrade_plan_health.rs \
  crates/gc_cli/tests/agent_authoring_bundle_guard.rs \
  crates/gc_cli/tests/pkg_low_semantic_boundary.rs \
  crates/gc_cli/tests/guard_extraction_fixtures.rs \
  crates/gc_cli/tests/large_workspace_agent_perf.rs \
  crates/gc_cli/tests/runtime_microbench_gpu_policy.rs \
  crates/gc_cli/tests/ai_stress_suite_fault_inject.rs \
  crates/gc_cli/tests/genesiscode_authoring_skill_guard.rs \
  crates/gc_cli/tests/ai_iteration_slo_regression.rs \
  crates/gc_cli/tests/default_iteration_workflow.rs; do
  if ! grep -Fq '#[ignore = "perf-gate"]' "$perf_gate_test"; then
    echo "test-execution-profile-matrix: $perf_gate_test must keep perf-gate ignore annotation" >&2
    exit 1
  fi
done

if ! grep -Fq 'GENESIS_TEST_CHANGED_BUDGET_MS:-120000' "$CHANGED_FAST_SCRIPT"; then
  echo "test-execution-profile-matrix: changed-fast default budget must remain 120000ms (2m)" >&2
  exit 1
fi

if ! grep -Fq 'GENESIS_BUDGET_CHANGED_FAST_MS:-120000' "$DEFAULT_LOOP_SCRIPT"; then
  echo "test-execution-profile-matrix: default iteration workflow budget must remain 120000ms (2m)" >&2
  exit 1
fi

if ! grep -Fq 'GENESIS_STRICT_GOLDEN_BUDGET_MS:-480000' "$STRICT_GOLDEN_SCRIPT"; then
  echo "test-execution-profile-matrix: strict-golden default budget must remain 480000ms (8m)" >&2
  exit 1
fi

if ! grep -Fq 'GENESIS_FULL_CROSS_HOST_BUDGET_MS:-720000' "$FULL_CROSS_HOST_RENDERER"; then
  echo "test-execution-profile-matrix: full-cross-host default budget must remain 720000ms (12m)" >&2
  exit 1
fi

if ! grep -Fq 'GENESIS_HEALTH_PREPUSH_BUDGET_MS:-480000' "$HEALTH_RENDERER"; then
  echo "test-execution-profile-matrix: prepush strict loop budget must remain pinned at GB-3 default 480000ms (8m)" >&2
  exit 1
fi
if ! grep -Fq 'CARGO_GATE_ENTRYPOINTS' "$HEALTH_RENDERER" || \
   ! grep -Fq 'gate["compilation"]' "$HEALTH_RENDERER"; then
  echo "test-execution-profile-matrix: health partitioning must consume gate-manifest compilation authority" >&2
  exit 1
fi

if ! grep -Fq 'GENESIS_HEALTH_RELEASE_FULL_BUDGET_MS:-1800000' "$HEALTH_RENDERER"; then
  echo "test-execution-profile-matrix: release-full strict loop budget must remain pinned at default 1800000ms (30m)" >&2
  exit 1
fi

if ! grep -Fq 'GENESIS_HEALTH_AGENT_INNER_LOOP_BUDGET_MS:-300000' "$HEALTH_RENDERER"; then
  echo "test-execution-profile-matrix: agent-inner-loop budget must remain pinned at default 300000ms (5m)" >&2
  exit 1
fi

if ! grep -Fq 'agent-inner-loop' "$HEALTH_RENDERER"; then
  echo "test-execution-profile-matrix: check_upgrade_plan_health.sh must support agent-inner-loop profile" >&2
  exit 1
fi

if ! grep -Fq 'default_health_shards_for_profile' "$HEALTH_RENDERER"; then
  echo "test-execution-profile-matrix: check_upgrade_plan_health.sh must keep deterministic shard default function" >&2
  exit 1
fi

if ! grep -Fq 'PROFILE_SHARDS' "$HEALTH_RENDERER"; then
  echo "test-execution-profile-matrix: check_upgrade_plan_health.sh must keep dedicated profile shard control" >&2
  exit 1
fi

if ! grep -Fq 'GENESIS_HEALTH_CARGO_GATE_SHARDS' "$HEALTH_RENDERER"; then
  echo "test-execution-profile-matrix: check_upgrade_plan_health.sh must keep dedicated cargo gate shard control" >&2
  exit 1
fi

if ! grep -Fq 'GENESIS_HEALTH_PROFILE_GATE_CACHE' "$HEALTH_RENDERER" || \
   ! grep -Fq 'apply_profile_gate_cache_policy' "$HEALTH_RENDERER" || \
   ! grep -Fq 'run_cached_health_gate.sh' "$HEALTH_RENDERER"; then
  echo "test-execution-profile-matrix: check_upgrade_plan_health.sh must keep deterministic profile gate cache wrapper policy" >&2
  exit 1
fi

if ! grep -Fq 'bash scripts/check_cargo_target_dir_policy.sh' "$HEALTH_RENDERER"; then
  echo "test-execution-profile-matrix: check_upgrade_plan_health.sh must run cargo target-dir policy conformance gate" >&2
  exit 1
fi

if ! grep -Fq 'genesis_configure_cargo_target_dir' "$HEALTH_RENDERER" || \
   ! grep -Fq 'root-host' "$HEALTH_RENDERER"; then
  echo "test-execution-profile-matrix: check_upgrade_plan_health.sh must resolve the root-host Cargo cache" >&2
  exit 1
fi

if ! grep -Fq 'partition_gate_commands' "$HEALTH_RENDERER"; then
  echo "test-execution-profile-matrix: check_upgrade_plan_health.sh must partition cargo vs non-cargo gate scheduling" >&2
  exit 1
fi

if ! grep -Fq 'GENESIS_HEALTH_REQUIRE_GPU_DEVICE_CONFORMANCE' "$HEALTH_RENDERER"; then
  echo "test-execution-profile-matrix: check_upgrade_plan_health.sh must keep explicit gpu device conformance lane toggle" >&2
  exit 1
fi

if ! grep -Fq 'export GENESIS_PERF_DISK_STRICT_MODE="1"' "$HEALTH_RENDERER" || \
   ! grep -Fq 'export GENESIS_RUNTIME_BACKEND_MATRIX_DISK_STRICT_MODE="1"' "$HEALTH_RENDERER"; then
  echo "test-execution-profile-matrix: strict profiles must force fail-closed disk headroom mode" >&2
  exit 1
fi

if ! grep -Fq 'sync-upgrade-plan-state: ok' "$UPGRADE_PLAN_SYNC_SCRIPT"; then
  echo "test-execution-profile-matrix: sync_upgrade_plan_state command must perform end-to-end synchronization checks" >&2
  exit 1
fi

if ! grep -Fq 'GENESIS_AGENT_GPU_PROFILE' "$HEALTH_RENDERER" || \
   ! grep -Fq 'genesis_apply_agent_gpu_profile_contract' "$HEALTH_RENDERER"; then
  echo "test-execution-profile-matrix: upgrade-plan health must enforce explicit agent gpu profile contract in automation contexts" >&2
  exit 1
fi

if ! grep -Fq 'agent-gpu-strict' "$AGENT_GPU_PROFILE_LIB" || \
   ! grep -Fq 'agent-gpu-fallback' "$AGENT_GPU_PROFILE_LIB"; then
  echo "test-execution-profile-matrix: agent gpu profile contract script must support strict and fallback profile selections" >&2
  exit 1
fi

if ! grep -Fq 'bash scripts/check_agent_scenario_perf.sh' "$HEALTH_RENDERER"; then
  echo "test-execution-profile-matrix: release-full profile must run agent scenario perf gate" >&2
  exit 1
fi

if ! grep -Fq 'bash scripts/check_wasm_production_surface.sh' "$HEALTH_RENDERER"; then
  echo "test-execution-profile-matrix: release-full profile must run wasm production surface isolation gate" >&2
  exit 1
fi

if ! grep -Fq 'bash scripts/check_large_workspace_agent_perf.sh' "$HEALTH_RENDERER"; then
  echo "test-execution-profile-matrix: release-full profile must run large-workspace agent perf gate" >&2
  exit 1
fi

if ! grep -Fq 'if [[ "$PROFILE" == "release-full" ]]; then' "$HEALTH_RENDERER" || \
   ! grep -Fq 'GPU_DEVICE_CONFORMANCE="1"' "$HEALTH_RENDERER"; then
  echo "test-execution-profile-matrix: release-full profile must require gpu device conformance by default" >&2
  exit 1
fi

if ! grep -Fq 'profile_runtime_budget.py' "$STRICT_GOLDEN_SCRIPT"; then
  echo "test-execution-profile-matrix: strict-golden script must emit/enforce runtime report via shared profile budget helper" >&2
  exit 1
fi

if ! grep -Fq 'strict-golden' "$STRICT_GOLDEN_SCRIPT"; then
  echo "test-execution-profile-matrix: strict-golden script must stamp strict-golden profile label into runtime report" >&2
  exit 1
fi

if ! grep -Fq 'profile_runtime_budget.py' "$WASM_CROSS_HOST_SCRIPT"; then
  echo "test-execution-profile-matrix: wasm cross-host script must emit/enforce runtime report via shared profile budget helper" >&2
  exit 1
fi

if ! grep -Fq 'wasm-cross-host' "$WASM_CROSS_HOST_SCRIPT"; then
  echo "test-execution-profile-matrix: wasm cross-host script must stamp wasm-cross-host profile label into runtime report" >&2
  exit 1
fi

if ! grep -Fq 'full-cross-host' "$FULL_CROSS_HOST_RENDERER"; then
  echo "test-execution-profile-matrix: full cross-host budget script must stamp full-cross-host profile label into runtime report" >&2
  exit 1
fi

if ! grep -Fq 'profile_runtime_budget.py' "$FULL_CROSS_HOST_RENDERER"; then
  echo "test-execution-profile-matrix: full cross-host budget script must emit/enforce aggregate runtime report via shared profile budget helper" >&2
  exit 1
fi

if ! grep -Fq 'enforce_prepush_history_budget' "$HEALTH_RENDERER" || \
   ! grep -Fq 'GENESIS_HEALTH_PREPUSH_MIN_HISTORY' "$HEALTH_RENDERER" || \
   ! grep -Fq 'GENESIS_HEALTH_PREPUSH_HISTORY_SCOPE_KEY' "$HEALTH_RENDERER"; then
  echo "test-execution-profile-matrix: prepush profile must enforce history-aware runtime budget controls" >&2
  exit 1
fi

if ! grep -Fq 'enforce_release_full_history_budget' "$HEALTH_RENDERER" || \
   ! grep -Fq 'GENESIS_HEALTH_RELEASE_FULL_MIN_HISTORY' "$HEALTH_RENDERER" || \
   ! grep -Fq 'GENESIS_HEALTH_RELEASE_FULL_HISTORY_SCOPE_KEY' "$HEALTH_RENDERER"; then
  echo "test-execution-profile-matrix: release-full profile must enforce history-aware runtime budget controls" >&2
  exit 1
fi

if ! grep -Fq 'GENESIS_HEALTH_RELEASE_FULL_BASELINE_HISTORY:-policies/perf/upgrade_plan_health_release_full_seed_history.jsonl' "$HEALTH_RENDERER"; then
  echo "test-execution-profile-matrix: release-full profile must default baseline seed history path" >&2
  exit 1
fi

if ! grep -Fq 'GENESIS_HEALTH_RELEASE_FULL_REQUIRE_MIN_HISTORY:-1' "$HEALTH_RENDERER"; then
  echo "test-execution-profile-matrix: release-full profile must fail-closed on insufficient history by default" >&2
  exit 1
fi

if ! grep -Fq 'GENESIS_HEALTH_STRICT_DISK_POLICY:-fail' "$HEALTH_RENDERER"; then
  echo "test-execution-profile-matrix: strict disk preflight policy default must remain fail-closed" >&2
  exit 1
fi

if ! grep -Fq 'bash scripts/check_roadmap_execution_manifest.sh' "$GREEN_FRONT_DOOR_SCRIPT"; then
  echo "test-execution-profile-matrix: green front door must enforce roadmap execution manifest drift" >&2
  exit 1
fi

if ! grep -Fq 'GENESIS_FULL_CROSS_HOST_BASELINE_HISTORY' "$FULL_CROSS_HOST_RENDERER"; then
  echo "test-execution-profile-matrix: full cross-host budget script must expose baseline seed history path" >&2
  exit 1
fi

if ! grep -Fq -- '--baseline-history "$EFFECTIVE_BASELINE_HISTORY"' "$FULL_CROSS_HOST_RENDERER"; then
  echo "test-execution-profile-matrix: full cross-host budget script must pass baseline history to shared runtime budget helper" >&2
  exit 1
fi

if ! grep -Fq -- '--require-min-history' "$FULL_CROSS_HOST_RENDERER"; then
  echo "test-execution-profile-matrix: full cross-host budget script must fail-closed on insufficient history depth" >&2
  exit 1
fi

if ! grep -Fq "$AGENT_SCENARIO_RENDERER" "$AGENT_SCENARIO_CHECK"; then
  echo "test-execution-profile-matrix: agent scenario check must delegate to the reviewed renderer" >&2
  exit 1
fi

if ! grep -Fq 'GENESIS_AGENT_SCENARIO_BASELINE_HISTORY' "$AGENT_SCENARIO_RENDERER"; then
  echo "test-execution-profile-matrix: agent scenario perf gate must expose baseline seed history path" >&2
  exit 1
fi

if ! grep -Fq 'GENESIS_AGENT_SCENARIO_REQUIRE_MIN_HISTORY' "$AGENT_SCENARIO_RENDERER"; then
  echo "test-execution-profile-matrix: agent scenario perf gate must expose minimum-history fail-closed control" >&2
  exit 1
fi

if ! grep -Fq "$AGENT_GENERATIVE_RENDERER" "$AGENT_GENERATIVE_CHECK"; then
  echo "test-execution-profile-matrix: agent generative check must delegate to the reviewed renderer" >&2
  exit 1
fi

if ! grep -Fq 'genesis/agent-generative-workloads-v0.1' "$AGENT_GENERATIVE_RENDERER"; then
  echo "test-execution-profile-matrix: agent generative workload gate must emit stable report kind" >&2
  exit 1
fi

if ! grep -Fq 'GENESIS_AGENT_GENERATIVE_SECONDARY_REPORT' "$AGENT_GENERATIVE_CHECK"; then
  echo "test-execution-profile-matrix: agent generative workload gate must support secondary report parity mode" >&2
  exit 1
fi

if grep -Fq '.genesis/perf/test_changed_fast_' "$CHANGED_FAST_SCRIPT" || \
   grep -Fq 'GENESIS_TEST_CHANGED_REPORT' "$CHANGED_FAST_SCRIPT" || \
   grep -Fq 'GENESIS_TEST_CHANGED_HISTORY' "$CHANGED_FAST_SCRIPT"; then
  echo "test-execution-profile-matrix: changed-fast default command must not own persistent metrics paths" >&2
  exit 1
fi
if ! grep -Fq '.genesis/perf/test_changed_fast_metrics.json' "$UPDATE_CHANGED_FAST_SCRIPT" || \
   ! grep -Fq '.genesis/perf/test_changed_fast_history.jsonl' "$UPDATE_CHANGED_FAST_SCRIPT"; then
  echo "test-execution-profile-matrix: explicit changed-fast updater must own canonical local E0 paths" >&2
  exit 1
fi

echo "test-execution-profile-matrix: ok"
