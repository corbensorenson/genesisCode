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
GC_AGENT_PROFILE_UPDATE="scripts/update_agent_authoring_bundle.sh"
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
KERNEL_TCB_SCRIPT="scripts/check_kernel_tcb_contract.sh"
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
HEALTH_EVIDENCE_RENDERER="scripts/render_health_profile_evidence_bundle.sh"
HEALTH_UPDATE_SCRIPT="scripts/update_upgrade_plan_health_report.sh"
RELEASE_MEASUREMENT_SCRIPT="scripts/measure_release_full_profile.sh"
RELEASE_MEASUREMENT_RUNNER="scripts/lib/release_full_measurement.py"
RELEASE_MEASUREMENT_SCHEMA="docs/spec/RELEASE_FULL_MEASUREMENT_v0.1.schema.json"
RELEASE_EVIDENCE_EXECUTION_SCRIPT="scripts/measure_release_evidence_v02.sh"
RELEASE_EVIDENCE_EXECUTION_RUNNER="scripts/lib/release_evidence_execution.py"
RELEASE_EVIDENCE_FANOUT_RUNNER="scripts/lib/release_evidence_fanout.py"
HOST_HANDLE_LIFECYCLE_EVIDENCE_RUNNER="scripts/lib/host_handle_lifecycle_evidence.py"
RELEASE_EVIDENCE_FANOUT_SCHEMA="docs/spec/RELEASE_EVIDENCE_FANOUT_AUTH_v0.2.schema.json"
RELEASE_EVIDENCE_WORKER_SCHEMA="docs/spec/RELEASE_EVIDENCE_WORKER_v0.2.schema.json"
RELEASE_EVIDENCE_AGGREGATE_SCHEMA="docs/spec/RELEASE_EVIDENCE_AGGREGATE_v0.2.schema.json"
RELEASE_EVIDENCE_DAG_POLICY="policies/release_evidence_dag_v0.2.json"
RELEASE_EVIDENCE_DAG_SCHEMA="docs/spec/RELEASE_EVIDENCE_DAG_v0.2.schema.json"
RELEASE_EVIDENCE_DAG_RUNNER="scripts/lib/release_evidence_dag.py"
REFERENCE_TARGET_PREPARE_SCRIPT="scripts/prepare_release_target_reference.sh"
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
GENERATED_AUTHORITY_UPDATE_SCRIPT="scripts/update_generated_authority.sh"
AGENT_GPU_PROFILE_CONTRACT_SCRIPT="scripts/check_agent_gpu_profile_contract.sh"
AGENT_GPU_PROFILE_LIB="scripts/lib/agent_gpu_profile_contract.sh"
WRITE_SKILL_DISTRIBUTION_SCRIPT="scripts/check_write_genesiscode_skill_distribution.sh"
HEALTH_EVIDENCE_LIB="scripts/lib/health_profile_evidence.py"
DENY_CONFIG="deny.toml"
LINT_SUPPRESSION_POLICY="scripts/lib/lint_suppression_policy.py"

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
  "$KERNEL_TCB_SCRIPT" \
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
  "$HEALTH_EVIDENCE_RENDERER" \
  "$HEALTH_UPDATE_SCRIPT" \
  "$RELEASE_MEASUREMENT_SCRIPT" \
  "$RELEASE_MEASUREMENT_RUNNER" \
  "$RELEASE_MEASUREMENT_SCHEMA" \
  "$RELEASE_EVIDENCE_EXECUTION_SCRIPT" \
  "$RELEASE_EVIDENCE_EXECUTION_RUNNER" \
  "$HOST_HANDLE_LIFECYCLE_EVIDENCE_RUNNER" \
  "$RELEASE_EVIDENCE_FANOUT_RUNNER" \
  "$RELEASE_EVIDENCE_FANOUT_SCHEMA" \
  "$RELEASE_EVIDENCE_WORKER_SCHEMA" \
  "$RELEASE_EVIDENCE_AGGREGATE_SCHEMA" \
  "$RELEASE_EVIDENCE_DAG_POLICY" \
  "$RELEASE_EVIDENCE_DAG_SCHEMA" \
  "$RELEASE_EVIDENCE_DAG_RUNNER" \
  "$REFERENCE_TARGET_PREPARE_SCRIPT" \
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
  "$GENERATED_AUTHORITY_UPDATE_SCRIPT" \
  "$AGENT_GPU_PROFILE_CONTRACT_SCRIPT" \
  "$AGENT_GPU_PROFILE_LIB" \
  "$WRITE_SKILL_DISTRIBUTION_SCRIPT" \
  "$HEALTH_EVIDENCE_LIB" \
  "$LINT_SUPPRESSION_POLICY" \
  "$DENY_CONFIG"; do
  [[ -f "$path" ]] || {
    echo "test-execution-profile-matrix: missing required file: $path" >&2
    exit 1
  }
done

python3 "$RELEASE_EVIDENCE_DAG_RUNNER" --root "$ROOT_DIR" check
python3 "$RELEASE_EVIDENCE_DAG_RUNNER" --root "$ROOT_DIR" self-test
python3 "$RELEASE_EVIDENCE_EXECUTION_RUNNER" --root "$ROOT_DIR" self-test
python3 "$HOST_HANDLE_LIFECYCLE_EVIDENCE_RUNNER" --self-test

require_doc_pattern() {
  local pattern="$1"
  if ! grep -Fq "$pattern" "$DOC"; then
    echo "test-execution-profile-matrix: missing profile matrix entry in $DOC: $pattern" >&2
    exit 1
  fi
}

require_ci_pattern() {
  local pattern="$1"
  if ! grep -Fq -- "$pattern" "$CI"; then
    echo "test-execution-profile-matrix: missing CI profile step in $CI: $pattern" >&2
    exit 1
  fi
}

require_doc_pattern '| `smoke` |'
require_doc_pattern '| `changed-fast` |'
require_doc_pattern '| `perf-gate-regressions` |'
require_doc_pattern '| `kernel-tail-stress` |'
require_doc_pattern '| `agent-inner-loop` |'
require_doc_pattern '| `release-full` |'
require_doc_pattern '| `strict-golden` |'
require_doc_pattern '| `full-cross-host` |'
require_doc_pattern '`<= 2m`'
require_doc_pattern '`<= 5m`'
require_doc_pattern '`<= 45m`'
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
require_doc_pattern 'scripts/measure_release_evidence_v02.sh'
require_doc_pattern 'genesis/release-evidence-worker-observation-v0.2'
require_doc_pattern 'three independently scheduled cold cache-sensitive workers'
require_doc_pattern 'genesis/release-evidence-aggregate-v0.2'
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
require_doc_pattern 'GENESIS_BUDGET_CHANGED_FAST_MS'
require_doc_pattern '20000ms contention envelope'
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
require_doc_pattern 'scripts/update_agent_authoring_bundle.sh profile'
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
require_doc_pattern 'Feature branches are validated by the `pull_request` event only'

require_ci_pattern 'Changed-File Fast Loop Budget'
require_ci_pattern '--dry-run'
require_ci_pattern 'The standard PR job already executes the full selected gate and test'
python3 - "$CI" <<'PY'
import pathlib
import re
import sys

source = pathlib.Path(sys.argv[1]).read_text(encoding="utf-8")
if not re.search(
    r"(?m)^on:\n  push:\n    branches:\n      - main\n  pull_request:\n",
    source,
):
    raise SystemExit(
        "test-execution-profile-matrix: branch pushes must run only on canonical main; "
        "pull requests own feature-branch validation"
    )
if not re.search(
    r"(?m)^concurrency:\n"
    r"  group: ci-\$\{\{ github\.event_name \}\}-\$\{\{ github\.event_name == 'pull_request' && github\.event\.pull_request\.number \|\| github\.event_name == 'push' && github\.sha \|\| github\.run_id \}\}\n"
    r"  cancel-in-progress: \$\{\{ github\.event_name == 'pull_request' \}\}\n",
    source,
):
    raise SystemExit(
        "test-execution-profile-matrix: only same-PR CI may supersede; push, schedule, and dispatch runs need unique groups"
    )
if "run-name: ci / ${{ github.event_name }} /" not in source:
    raise SystemExit("test-execution-profile-matrix: CI run names must bind event and selected profile")
PY
require_ci_pattern 'Docs Quickstart Gate'
require_ci_pattern 'bash scripts/check_docs_quickstart.sh'
require_ci_pattern 'Ignored Perf Gate Regression Tests'
require_ci_pattern 'bash scripts/test_perf_gates.sh'
require_ci_pattern 'GENESIS_HEALTH_DEV_FAST_WALL_BUDGET_MS=450000'
require_ci_pattern 'GENESIS_HEALTH_PROFILE=dev-fast'
require_ci_pattern 'bash scripts/test_perf_gates.sh --exclude-test upgrade_plan_health'
require_ci_pattern 'release_full_measurement:'
require_ci_pattern 'release_evidence_cold_worker:'
require_ci_pattern 'release_evidence_warm_worker:'
require_ci_pattern 'release_evidence_invariant_worker:'
require_ci_pattern 'release_evidence_stress_worker:'
require_ci_pattern 'Publish Same-Run Cold-1 Fanout'
require_ci_pattern 'Aggregate Release Evidence DAG'
require_ci_pattern 'Local Workspace Test Contract (CI unset)'
require_ci_pattern 'env -u CI cargo test --workspace --profile selfhost-strict'
python3 - "$CI" <<'PY'
import pathlib
import sys

source = pathlib.Path(sys.argv[1]).read_text(encoding="utf-8")
job_start = source.find("  local_workspace_test_contract:\n")
aggregate_start = source.find("  test:\n", job_start)
aggregate_end = source.find("  pr_strict_equivalence_gate:\n", aggregate_start)
if min(job_start, aggregate_start, aggregate_end) < 0:
    raise SystemExit(
        "test-execution-profile-matrix: isolated local-workspace or required aggregate job is missing"
    )
if source.count("- name: Local Workspace Test Contract (CI unset)") != 1:
    raise SystemExit(
        "test-execution-profile-matrix: CI-unset workspace contract must have one authoritative lane"
    )
local = source[job_start:aggregate_start]
aggregate = source[aggregate_start:aggregate_end]
for required in (
    "runs-on: ubuntu-latest",
    "fetch-depth: 0",
    "Local Workspace Disk Headroom",
    "--min-kb 10485760 --strict 1",
    "cargo fetch --locked",
    "env -u CI cargo test --workspace --profile selfhost-strict --locked --offline",
):
    if required not in local:
        raise SystemExit(
            f"test-execution-profile-matrix: isolated local-workspace job is missing {required!r}"
        )
if "Playwright" in local or "test_perf_gates.sh" in local:
    raise SystemExit(
        "test-execution-profile-matrix: isolated local-workspace job accumulates unrelated heavy artifacts"
    )
for required in (
    "if: ${{ always() }}",
    "- test_suite",
    "- local_workspace_test_contract",
    "- release_full_measurement",
    "TEST_SUITE_RESULT: ${{ needs.test_suite.result }}",
    "LOCAL_WORKSPACE_RESULT: ${{ needs.local_workspace_test_contract.result }}",
    "RELEASE_FULL_MEASUREMENT_RESULT: ${{ needs.release_full_measurement.result }}",
    "RELEASE_FULL_MEASUREMENT_REQUIRED:",
    "Required CI Aggregate",
):
    if required not in aggregate:
        raise SystemExit(
            f"test-execution-profile-matrix: protected test aggregate is missing {required!r}"
        )
PY
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
require_ci_pattern 'GENESIS_BUDGET_CHANGED_FAST_MS=20000'
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
  crates/gc_cli/tests/cli_agent_benchmark_scoring.rs \
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

for scoring_matrix_test in \
  public_references_score_perfectly_and_deterministically_with_shipped_binary \
  scoring_fails_closed_or_penalizes_independent_adversarial_candidates; do
  if ! grep -B 2 -F "fn ${scoring_matrix_test}" \
    crates/gc_cli/tests/cli_agent_benchmark_scoring.rs | \
    grep -Fq '#[ignore = "perf-gate"]'; then
    echo "test-execution-profile-matrix: scoring matrix ${scoring_matrix_test} must remain in the required serial perf lane" >&2
    exit 1
  fi
done
if ! grep -Fq 'cli_agent_benchmark_scoring' "$PERF_GATES_SCRIPT" || \
   ! grep -Fq 'GENESIS_SCORING_MATRIX_BUDGET_MS' "$PERF_GATES_SCRIPT" || \
   ! grep -Fq 'scorer_process_timeout_ms=30000' "$PERF_GATES_SCRIPT"; then
  echo "test-execution-profile-matrix: scoring matrix must retain its serial trigger and finite resource envelope" >&2
  exit 1
fi
if grep -B 2 -F 'fn scoring_contract_core_accepts_reference_and_rejects_symlinks_before_execution' \
  crates/gc_cli/tests/cli_agent_benchmark_scoring.rs | \
  grep -Fq '#[ignore = "perf-gate"]'; then
  echo "test-execution-profile-matrix: scoring contract core must remain in the default lane" >&2
  exit 1
fi

if ! grep -B 2 -F 'fn changed_fast_defaults_to_temporary_metrics_and_ignores_legacy_output_env' \
  crates/gc_cli/tests/changed_fast_perf_regressions.rs | grep -Fq '#[ignore = "perf-gate"]'; then
  echo "test-execution-profile-matrix: nested changed-fast probe must remain outside the default Rust suite" >&2
  exit 1
fi
if ! grep -Fq 'changed_fast_perf_regressions' "$PERF_GATES_SCRIPT"; then
  echo "test-execution-profile-matrix: changed-fast perf regressions must remain in the serial perf lane" >&2
  exit 1
fi

for stress_test in \
  spawn_per_op_timeout_kills_bridge_processes_and_recovers \
  persistent_stdio_timeout_kills_process_trees_and_workers; do
  if ! grep -B 2 -F "fn ${stress_test}" crates/gc_effects/src/runner_host_bridge_tests.rs | \
    grep -Fq '#[ignore = "stress-gate"]'; then
    echo "test-execution-profile-matrix: ${stress_test} must remain in the dedicated stress lane" >&2
    exit 1
  fi
  if ! grep -Fq "runner_host_bridge::tests::${stress_test} --quiet -- --ignored --exact" \
    scripts/render_host_bridge_fault_injection_report.sh; then
    echo "test-execution-profile-matrix: host-bridge renderer must execute ignored stress test ${stress_test}" >&2
    exit 1
  fi
done

if ! grep -Fq \
  'runner_process_control::tests::zombie_only_process_group_is_execution_quiescent --quiet -- --exact' \
  scripts/render_host_bridge_fault_injection_report.sh; then
  echo "test-execution-profile-matrix: host-bridge renderer must exercise zombie-only process-group quiescence" >&2
  exit 1
fi
if ! grep -Fq \
  'runner_host_bridge::runner_host_bridge_persistent::tests::persistent_stop_is_bounded_when_signal_and_reap_fail --quiet -- --exact' \
  scripts/render_host_bridge_fault_injection_report.sh; then
  echo "test-execution-profile-matrix: host-bridge renderer must exercise bounded signal/reap failure" >&2
  exit 1
fi
for probe_marker in \
  'HOST_PLATFORM="darwin"' \
  'PROCESS_GROUP_PROBE="libproc-pgrp-status"' \
  'HOST_PLATFORM="linux"' \
  'PROCESS_GROUP_PROBE="procfs-pgrp-status"'; do
  if ! grep -Fq "$probe_marker" scripts/render_host_bridge_fault_injection_report.sh; then
    echo "test-execution-profile-matrix: host-bridge renderer is missing probe marker $probe_marker" >&2
    exit 1
  fi
done

for lifecycle_probe in \
  'browser_xr::first_party_browser_and_xr_reject_repeated_close' \
  'editor_first_party_core_ops_are_replayable_without_bridge' \
  'runner_gfx_host::lifecycle_tests::runtime_drop_reaps_only_owned_desktop_surfaces' \
  'model_lifecycle::model_runner_plugin_session_is_owned_reaped_and_restart_isolated' \
  'scripts/lib/host_bridge_daemon_lifecycle.py' \
  'host_bridge_daemon_lifecycle.py --self-test' \
  'warm-daemon-provider-success-error-timeout-restart-shutdown-eof' \
  '"daemon_service_lifecycle"' \
  '"fresh_daemon_process_isolation": daemon_verified' \
  '"no_live_provider_or_descendant": daemon_verified' \
  '--features gfx-desktop-backend' \
  '--features gpu-device-backend' \
  'device_runtime_resources_are_scoped_and_reaped' \
  '"coverage_complete": False' \
  '"status": "bridge-profile-implemented"' \
  '"standard_model_api_owner": "R5.4.e"'; do
  if ! grep -Fq -- "$lifecycle_probe" scripts/render_host_bridge_fault_injection_report.sh; then
    echo "test-execution-profile-matrix: host lifecycle renderer is missing probe $lifecycle_probe" >&2
    exit 1
  fi
done

for macos_lifecycle_probe in \
  'release-target-reference-host-lifecycle' \
  'scripts/lib/host_bridge_daemon_lifecycle.py' \
  'GENESIS_HOST_BRIDGE_DAEMON_LIFECYCLE_REPORT' \
  'reference-target-ios/ios/host_bridge_daemon_lifecycle_report.json'; do
  if ! grep -Fq "$macos_lifecycle_probe" scripts/prepare_release_target_reference.sh; then
    echo "test-execution-profile-matrix: macOS reference lane is missing lifecycle probe $macos_lifecycle_probe" >&2
    exit 1
  fi
done

if ! grep -B 2 -F 'fn task_cards_python_and_planner_selection_remain_stable_under_parallel_load' \
  crates/gc_cli/tests/cli_agent_plan.rs | grep -Fq '#[ignore = "stress-gate"]' || \
   ! grep -Fq 'task_cards_python_and_planner_selection_remain_stable_under_parallel_load' \
  scripts/check_gc_agent_task_cards.sh || \
   ! grep -Fq -- '--locked -- --ignored --exact' scripts/check_gc_agent_task_cards.sh; then
  echo "test-execution-profile-matrix: agent-plan parallel parity must remain in the dedicated agent-card gate" >&2
  exit 1
fi

if ! grep -B 2 -F 'fn tail_loop_ten_million_iterations_has_constant_evaluator_depth' \
  crates/gc_kernel/src/tests.rs | grep -Fq '#[ignore = "stress-gate"]' || \
   ! grep -Fq 'tests::tail_loop_ten_million_iterations_has_constant_evaluator_depth' \
  "$PERF_GATES_SCRIPT" || \
   ! grep -Fq -- '--ignored' "$PERF_GATES_SCRIPT" || \
   ! grep -Fq 'GENESIS_KERNEL_TAIL_STRESS_BUDGET_MS:-300000' \
  "$PERF_GATES_SCRIPT" || \
   ! grep -Fq 'GENESIS_KERNEL_TAIL_STRESS_DISK_BUDGET_BYTES:-536870912' \
  "$PERF_GATES_SCRIPT" || \
   ! grep -Fq 'bash scripts/check_kernel_tcb_contract.sh' "$PERF_GATES_SCRIPT" || \
   ! grep -Fq 'bash scripts/test_perf_gates.sh --kernel-tail-stress' "$HEALTH_RENDERER"; then
  echo "test-execution-profile-matrix: kernel 10M tail proof must remain in the bounded release-full stress lane" >&2
  exit 1
fi

if ! grep -Fq 'GENESIS_TEST_CHANGED_BUDGET_MS:-120000' "$CHANGED_FAST_SCRIPT"; then
  echo "test-execution-profile-matrix: changed-fast default budget must remain 120000ms (2m)" >&2
  exit 1
fi
if ! grep -Fq 'GENESIS_TEST_CHANGED_FALLBACK_BUDGET_MS:-720000' "$CHANGED_FAST_SCRIPT" || \
   ! grep -Fq 'GENESIS_CHANGED_GATE_FALLBACK_DISK_BUDGET_BYTES=3221225472' "$CHANGED_FAST_SCRIPT"; then
  echo "test-execution-profile-matrix: changed-fast prepush fallback must use the GB-3 12m/3GiB envelope" >&2
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

if ! grep -Fq 'GENESIS_HEALTH_PREPUSH_BUDGET_MS:-720000' "$HEALTH_RENDERER"; then
  echo "test-execution-profile-matrix: prepush strict loop budget must remain pinned at GB-3 default 720000ms (12m)" >&2
  exit 1
fi
if ! grep -Fq 'CARGO_GATE_ENTRYPOINTS' "$HEALTH_RENDERER" || \
   ! grep -Fq 'gate["compilation"]' "$HEALTH_RENDERER"; then
  echo "test-execution-profile-matrix: health partitioning must consume gate-manifest compilation authority" >&2
  exit 1
fi

if ! grep -Fq 'GENESIS_HEALTH_RELEASE_FULL_BUDGET_MS:-2700000' "$HEALTH_RENDERER"; then
  echo "test-execution-profile-matrix: release-full strict loop budget must remain pinned at the GB-4 ceiling 2700000ms (45m)" >&2
  exit 1
fi

if ! grep -Fq 'RUNTIME_BACKEND_BUDGET_MS=360000' "$HEALTH_EVIDENCE_RENDERER" || \
   ! grep -Fq 'if [[ "$PROFILE" == "release-full" ]]; then' "$HEALTH_EVIDENCE_RENDERER" || \
   ! grep -Fq 'RUNTIME_BACKEND_BUDGET_MS=600000' "$HEALTH_EVIDENCE_RENDERER" || \
   ! grep -Fq 'GENESIS_RUNTIME_BACKEND_MATRIX_BUDGET_MS="$RUNTIME_BACKEND_BUDGET_MS"' "$HEALTH_EVIDENCE_RENDERER"; then
  echo "test-execution-profile-matrix: the cold release runtime matrix must use the declared 600s gate envelope without relaxing the 360s prepush bound" >&2
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

if ! grep -Fq 'generated_authority.py --update' "$GENERATED_AUTHORITY_UPDATE_SCRIPT"; then
  echo "test-execution-profile-matrix: canonical generated-authority updater must delegate to the transactional graph runner" >&2
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

python3 - "$ROOT_DIR" <<'PY'
import json
import pathlib
import re
import sys

root = pathlib.Path(sys.argv[1])
policy_path = root / "policies/release_target_reference_set_v0.1.json"
schema_path = root / "docs/spec/RELEASE_TARGET_REFERENCE_SET_v0.1.schema.json"
evidence_schema_path = root / "docs/spec/HEALTH_PROFILE_EVIDENCE_BUNDLE_v0.2.schema.json"
measurement_schema_path = root / "docs/spec/RELEASE_FULL_MEASUREMENT_v0.1.schema.json"
pair_measurement_schema_path = root / "docs/spec/RELEASE_FULL_MEASUREMENT_PAIR_v0.1.schema.json"
worker_schema_path = root / "docs/spec/RELEASE_EVIDENCE_WORKER_v0.2.schema.json"
fanout_schema_path = root / "docs/spec/RELEASE_EVIDENCE_FANOUT_AUTH_v0.2.schema.json"
aggregate_schema_path = root / "docs/spec/RELEASE_EVIDENCE_AGGREGATE_v0.2.schema.json"
policy = json.loads(policy_path.read_text(encoding="utf-8"))
schema = json.loads(schema_path.read_text(encoding="utf-8"))
evidence_schema = json.loads(evidence_schema_path.read_text(encoding="utf-8"))
measurement_schema = json.loads(measurement_schema_path.read_text(encoding="utf-8"))
pair_measurement_schema = json.loads(pair_measurement_schema_path.read_text(encoding="utf-8"))
worker_schema = json.loads(worker_schema_path.read_text(encoding="utf-8"))
fanout_schema = json.loads(fanout_schema_path.read_text(encoding="utf-8"))
aggregate_schema = json.loads(aggregate_schema_path.read_text(encoding="utf-8"))
if schema.get("$id") != "https://genesiscode.dev/schemas/release-target-reference-set-v0.1.json":
    raise SystemExit("test-execution-profile-matrix: release target reference schema id mismatch")
if evidence_schema.get("$id") != "https://genesiscode.dev/schemas/health-profile-evidence-bundle-v0.2.json":
    raise SystemExit("test-execution-profile-matrix: health evidence schema id mismatch")
if measurement_schema.get("$id") != "https://genesiscode.dev/schemas/release-full-measurement-v0.1.schema.json":
    raise SystemExit("test-execution-profile-matrix: release measurement schema id mismatch")
if pair_measurement_schema.get("$id") != "https://genesiscode.dev/schemas/release-full-measurement-pair-v0.1.schema.json":
    raise SystemExit("test-execution-profile-matrix: release pair measurement schema id mismatch")
if worker_schema.get("$id") != "https://genesiscode.dev/schemas/release-evidence-worker-v0.2.schema.json":
    raise SystemExit("test-execution-profile-matrix: release evidence worker schema id mismatch")
if fanout_schema.get("$id") != "https://genesiscode.dev/schemas/release-evidence-fanout-auth-v0.2.schema.json":
    raise SystemExit("test-execution-profile-matrix: release evidence fanout schema id mismatch")
if aggregate_schema.get("$id") != "https://genesiscode.dev/schemas/release-evidence-aggregate-v0.2.schema.json":
    raise SystemExit("test-execution-profile-matrix: release evidence aggregate schema id mismatch")
if "hostHandleLifecycle" not in aggregate_schema.get("required", []):
    raise SystemExit("test-execution-profile-matrix: release aggregate schema omits host lifecycle closure")
host_lifecycle_schema = aggregate_schema.get("$defs", {}).get("hostHandleLifecycle", {})
if host_lifecycle_schema.get("additionalProperties") is not False:
    raise SystemExit("test-execution-profile-matrix: host lifecycle aggregate schema is open")
if policy.get("kind") != "genesis/release-target-reference-set-v0.1":
    raise SystemExit("test-execution-profile-matrix: release target reference policy kind mismatch")
if policy.get("lifecycleSteps") != ["install", "launch", "smoke", "teardown", "reap"]:
    raise SystemExit("test-execution-profile-matrix: authentic lifecycle steps drift")
shards = policy.get("shards")
if not isinstance(shards, list) or [row.get("target") for row in shards] != ["android", "edge", "ios", "service-runtime"]:
    raise SystemExit("test-execution-profile-matrix: named target shards must remain complete and sorted")
required = {
    "commandEnv", "expectedOutcome", "identityEnv", "identityProbe", "productId",
    "referenceCommand", "runner", "runtimeClass", "sdkIdentityEnv", "sdkIdentityProbe", "target",
}
for row in shards:
    if set(row) != required or row["expectedOutcome"] != "unsupported-product":
        raise SystemExit(f"test-execution-profile-matrix: invalid named target shard: {row.get('target')}")
    if "PINNED" in row["referenceCommand"] or "PINNED" in row["sdkIdentityProbe"]:
        raise SystemExit(f"test-execution-profile-matrix: placeholder reference integration: {row.get('target')}")
shards_by_target = {row["target"]: row for row in shards}
android_command = shards_by_target["android"]["referenceCommand"].lower()
for marker in ["bundletool", "install-apks", '--device-id="$(adb get-serialno)"']:
    if marker not in android_command:
        raise SystemExit(f"test-execution-profile-matrix: Android reference command binding missing: {marker}")

health = (root / "scripts/render_upgrade_plan_health_report.sh").read_text(encoding="utf-8")
workflow = (root / ".github/workflows/ci.yml").read_text(encoding="utf-8")
node_setup_surfaces = [workflow]
node_setup_surfaces.extend(
    path.read_text(encoding="utf-8")
    for path in sorted((root / ".github/actions").glob("*/action.yml"))
)
node_versions = [
    version
    for surface in node_setup_surfaces
    for version in re.findall(r"(?m)^\s+node-version: ([^\s]+)\s*$", surface)
]
if not node_versions or set(node_versions) != {"22.23.2"}:
    raise SystemExit(
        "test-execution-profile-matrix: every hosted Node setup must use exact pin 22.23.2"
    )
for marker in [
    "GENESIS_HEALTH_EVIDENCE_REQUIRED=1",
    "GENESIS_HEALTH_EVIDENCE_MANIFEST=",
    "GENESIS_GCPM_TARGET_RUNTIME_EXPECT_OUTCOME=unsupported-product",
    "GENESIS_AGENT_PARITY_PREBUILT_REPORT=",
    "GENESIS_AGENT_GENERATIVE_PREBUILT_REPORT=",
    "GENESIS_GPU_XR_PRODUCTIZATION_PREBUILT_REPORT=",
    "GENESIS_CHECK_HOST_BRIDGE_FAULT_REPORT=",
    "GENESIS_WRITE_SKILL_DIST_VERIFY_RUNTIME=1",
]:
    if marker not in health:
        raise SystemExit(f"test-execution-profile-matrix: release evidence marker missing: {marker}")
try:
    release_section = health.rsplit("  release-full)\n", 1)[1].split(
        "  full-selfhost-cutover)\n", 1
    )[0]
except IndexError as exc:
    raise SystemExit("test-execution-profile-matrix: release profile topology is not parseable") from exc
if "bash scripts/check_write_genesiscode_skill_conformance.sh" in release_section:
    raise SystemExit(
        "test-execution-profile-matrix: release profile duplicates write-skill conformance"
    )
for marker in [
    "GENESIS_WRITE_SKILL_GAUNTLET_REPORT='$HEALTH_EVIDENCE_ROOT/agent_capability_gauntlet_report.json'",
    "GENESIS_WRITE_SKILL_GENERATIVE_REPORT='$HEALTH_EVIDENCE_ROOT/agent_generative_workloads_report.json'",
    "GENESIS_WRITE_SKILL_RUNTIME_BACKEND_REPORT='$HEALTH_EVIDENCE_ROOT/runtime_backend_feature_matrix_report.json'",
    "GENESIS_WRITE_SKILL_HOST_BRIDGE_REPORT='$HEALTH_EVIDENCE_ROOT/host_bridge_fault_injection_report.json'",
    "GENESIS_WRITE_SKILL_GPU_XR_REPORT='$HEALTH_EVIDENCE_ROOT/gpu_xr_productization_kits_report.json'",
    "GENESIS_WRITE_SKILL_ASSURANCE_REPORT='$HEALTH_EVIDENCE_ROOT/assurance_profile_packs_report.json'",
    "bash scripts/check_write_genesiscode_skill_distribution.sh",
]:
    if marker not in release_section:
        raise SystemExit(
            f"test-execution-profile-matrix: release distribution binding missing: {marker}"
        )
for marker in [
    "release_target_reference_readiness:",
    "release_evidence_cold_worker:",
    "release_evidence_warm_worker:",
    "release_evidence_invariant_worker:",
    "release_evidence_stress_worker:",
    "release_full_measurement:",
    "macos-15",
    "ubuntu-24.04",
    "GENESIS_GCPM_TARGET_RUNTIME_RUNNER_LABEL",
    "GENESIS_GCPM_TARGET_RUNTIME_REQUIRE_REFERENCE_SETUP",
    "GENESIS_GCPM_TARGET_RUNTIME_TARGETS",
    "prepare_release_target_reference.sh",
    "android-emulator-runner@v2",
    "Enable Android KVM",
    "wasmtime/setup@v1",
    "--exclude-test upgrade_plan_health",
    "index: [1, 2, 3]",
    "--state cold",
    "--state warm",
    "--evidence-class invariant",
    "--evidence-class stress-performance",
    "release-evidence-fanout-${{ github.run_id }}-${{ github.run_attempt }}-${{ github.sha }}",
    "scripts/lib/release_evidence_fanout.py",
    "scripts/measure_release_evidence_v02.sh aggregate",
    "scripts/measure_release_evidence_v02.sh initialize-worker",
    "orchestration/fanout.stderr.log",
    "if-no-files-found: error",
    "--target-report .genesis/reference-targets/reference-target-android.json",
    "--worker-output .genesis/release-workers/release-evidence-worker-cold-1",
    "--worker-output .genesis/release-workers/release-evidence-worker-stress-3",
    "RELEASE_FULL_MEASUREMENT_REQUIRED",
    "required release_full_measurement result",
    "gpu_runner_preflight:",
    "Collect Exact Repository Runner Inventory",
    "Classify Exact Runner Readiness",
    "Enforce Requested Runner Readiness",
    "policies/ci_control_plane_v0.1.json",
    "scripts/lib/ci_runner_preflight.py",
    "ci-runner-preflight-${{ github.run_id }}-${{ github.run_attempt }}",
    "infrastructure-failure",
    "unsupported-profile",
]:
    if marker not in workflow:
        raise SystemExit(f"test-execution-profile-matrix: named target CI marker missing: {marker}")

def workflow_job(name):
    match = re.search(
        rf"(?ms)^  {re.escape(name)}:\n(?P<section>.*?)(?=^  [A-Za-z0-9_]+:\n|\Z)",
        workflow,
    )
    if match is None:
        raise SystemExit(f"test-execution-profile-matrix: missing CI job: {name}")
    return match.group("section")

release_worker_jobs = [
    "release_evidence_cold_worker",
    "release_evidence_warm_worker",
    "release_evidence_invariant_worker",
    "release_evidence_stress_worker",
]
for job in release_worker_jobs:
    section = workflow_job(job)
    if "needs:" in section or "release_target_reference_readiness" in section:
        raise SystemExit(
            f"test-execution-profile-matrix: {job} must start independently of target readiness"
        )
aggregate_section = workflow_job("release_full_measurement")
for dependency in (
    "- release_target_reference_readiness",
    "- release_evidence_cold_worker",
    "- release_evidence_warm_worker",
    "- release_evidence_invariant_worker",
    "- release_evidence_stress_worker",
):
    if dependency not in aggregate_section:
        raise SystemExit(
            f"test-execution-profile-matrix: release aggregate lacks dependency: {dependency}"
        )

policy_control = json.loads((root / "policies/ci_control_plane_v0.1.json").read_text(encoding="utf-8"))
if policy_control.get("kind") != "genesis/ci-control-plane-policy-v0.1":
    raise SystemExit("test-execution-profile-matrix: CI control-plane policy identity mismatch")
if policy_control.get("limitsSeconds") != {
    "latestMainPushDisposition": 7200,
    "standardRunDisposition": 7200,
    "fullRunTermination": 3600,
    "successfulFullFreshness": 172800,
    "scheduledFullCadence": 93600,
    "runnerPreflight": 300,
}:
    raise SystemExit("test-execution-profile-matrix: CI control-plane limit drift")
lanes = policy_control.get("runnerLanes")
expected_lanes = {
    "primary-linux": ["self-hosted", "linux", "x64", "gpu"],
    "nvidia-linux": ["self-hosted", "linux", "x64", "gpu", "nvidia"],
    "amd-linux": ["self-hosted", "linux", "x64", "gpu", "amd"],
    "intel-windows": ["self-hosted", "windows", "x64", "gpu", "intel"],
    "apple-macos": ["self-hosted", "macOS", "arm64", "gpu", "apple"],
}
if not isinstance(lanes, list) or {row.get("id"): row.get("requiredLabels") for row in lanes} != expected_lanes:
    raise SystemExit("test-execution-profile-matrix: exact self-hosted runner labels drift")

for job, dispatch in {
    "gpu_device_microbench": "primary_linux_dispatch",
    "gpu_device_microbench_nvidia_linux": "nvidia_linux_dispatch",
    "gpu_device_microbench_amd_linux": "amd_linux_dispatch",
    "gpu_device_microbench_intel_windows": "intel_windows_dispatch",
    "gpu_device_microbench_apple_macos": "apple_macos_dispatch",
}.items():
    start = workflow.find(f"  {job}:\n")
    next_job = re.search(r"(?m)^  [A-Za-z0-9_]+:\n", workflow[start + len(job) + 4 :])
    end = len(workflow) if next_job is None else start + len(job) + 4 + next_job.start()
    section = workflow[start:end]
    if start < 0 or "needs: gpu_runner_preflight" not in section or f"outputs.{dispatch} == 'true'" not in section:
        raise SystemExit(f"test-execution-profile-matrix: {job} bypasses hosted exact-label preflight")

for job, timeout in {
    "gpu_runner_preflight": "timeout-minutes: 5",
    "release_target_reference_readiness": "timeout-minutes: 20",
    "release_evidence_cold_worker": "timeout-minutes: 55",
    "release_evidence_warm_worker": "timeout-minutes: 55",
    "release_evidence_invariant_worker": "timeout-minutes: 55",
    "release_evidence_stress_worker": "timeout-minutes: 55",
    "release_full_measurement": "timeout-minutes: 5",
    "local_workspace_test_contract": "timeout-minutes: 45",
    "test": "timeout-minutes: 5",
    "webxr_browser_conformance": "timeout-minutes: 20",
    "gpu_device_microbench": "timeout-minutes: 45",
    "gpu_device_microbench_deterministic": "timeout-minutes: 45",
    "gpu_device_conformance_release_gate": "timeout-minutes: 5",
    "gpu_device_microbench_nvidia_linux": "timeout-minutes: 45",
    "gpu_device_microbench_amd_linux": "timeout-minutes: 45",
    "gpu_device_microbench_intel_windows": "timeout-minutes: 45",
    "gpu_device_microbench_apple_macos": "timeout-minutes: 45",
    "gpu_device_conformance_matrix_gate": "timeout-minutes: 5",
}.items():
    match = re.search(
        rf"(?ms)^  {re.escape(job)}:\n(?P<section>.*?)(?=^  [A-Za-z0-9_]+:\n|\Z)",
        workflow,
    )
    if match is None or timeout not in match.group("section"):
        raise SystemExit(
            f"test-execution-profile-matrix: {job} lacks the reviewed full-run timeout {timeout}"
        )
test_suite = re.search(
    r"(?ms)^  test_suite:\n(?P<section>.*?)(?=^  [A-Za-z0-9_]+:\n|\Z)",
    workflow,
)
expected_test_timeout = (
    "timeout-minutes: ${{ (github.event_name == 'schedule' || "
    "(github.event_name == 'workflow_dispatch' && github.event.inputs.profile == 'full')) "
    "&& 45 || 120 }}"
)
if test_suite is None or expected_test_timeout not in test_suite.group("section"):
    raise SystemExit("test-execution-profile-matrix: full test lane is not bounded to 45 minutes")
test_suite_section = test_suite.group("section")
for marker in [
    "'[\"governance\",\"runtime\",\"platform\"]'",
    "GENESIS_CI_LANE: ${{ matrix.lane }}",
    "governance|runtime|platform",
]:
    if marker not in test_suite_section:
        raise SystemExit(f"test-execution-profile-matrix: full test sharding marker missing: {marker}")

lane_start = {
    "Prerequisite Manifest Guard": "governance",
    "Format": "governance",
    "Clippy": "runtime",
    "Install Node (Release Evidence + WASM)": "platform",
    "Ignored Perf Gate Regression Tests": "platform",
    "Upload Test Shard Artifacts": "runtime",
    "Performance Budgets": "platform",
}
lane_end = {
    "Dependency Mirror Contract Guard",
    "Test Size Budget Guard",
    "Changed-File Fast Loop Budget",
    "Install Playwright Chromium (Release Evidence)",
    "Ignored Perf Gate Regression Tests",
    "Upload Test Shard Artifacts",
    "WASI Build + Smoke (genesis_wasi.wasm)",
}
named_steps = list(re.finditer(r"(?m)^      - name: (?P<name>.+)$", test_suite_section))
step_blocks = {}
lane = None
for index, step in enumerate(named_steps):
    name = step.group("name")
    end = named_steps[index + 1].start() if index + 1 < len(named_steps) else len(test_suite_section)
    block = test_suite_section[step.start():end]
    step_blocks[name] = block
    if name in lane_start:
        lane = lane_start[name]
    if lane is not None:
        required = (
            "env.GENESIS_CI_LANE == 'standard' || "
            f"env.GENESIS_CI_LANE == '{lane}'"
        )
        if required not in block:
            raise SystemExit(
                f"test-execution-profile-matrix: {name} is not owned by standard+{lane}"
            )
    if name in lane_end:
        lane = None

release_dag_owned_steps = {
    "Runtime Backend Feature Matrix Guard",
    "Performance Budgets",
    "AI Iteration SLO",
    "AI Stress Suite (Tasks + Bridge + GPU/Compute)",
    "Hot Path Budgets",
    "Runtime Microbench Budgets",
    "GPU Compute Runtime Profile (Compute-Only)",
    "Agent Capability Gauntlet (Selfhost-Only)",
    "Agent End-to-End Scenario Perf Gate",
    "Agent Generative Workload Gate",
}
for name in sorted(release_dag_owned_steps):
    block = step_blocks.get(name, "")
    if "env.GENESIS_CI_PROFILE == 'standard' &&" not in block:
        raise SystemExit(
            f"test-execution-profile-matrix: {name} is not standard-only outside the full DAG"
        )
    if "GENESIS_CI_PROFILE == \"full\"" in block or "GENESIS_CI_PROFILE == 'full'" in block:
        raise SystemExit(
            f"test-execution-profile-matrix: {name} duplicates a release-evidence DAG producer"
        )

watchdog = (root / ".github/workflows/ci-watchdog.yml").read_text(encoding="utf-8")
for marker in [
    "name: ci-watchdog",
    'cron: "17 * * * *"',
    "actions: read",
    "group: ci-watchdog-${{ github.event_name }}-${{ github.run_id }}",
    "timeout-minutes: 5",
    "actions/workflows/ci.yml/runs?branch=main&per_page=100",
    "scripts/lib/ci_liveness_watchdog.py evaluate",
    '--expected-head "$(git rev-parse HEAD)"',
    "ci-liveness-disposition-${{ github.run_id }}-${{ github.run_attempt }}",
]:
    if marker not in watchdog:
        raise SystemExit(f"test-execution-profile-matrix: independent CI watchdog marker missing: {marker}")
if "group: ci-${{" in watchdog or "cancel-in-progress: true" in watchdog:
    raise SystemExit("test-execution-profile-matrix: watchdog shares CI supersession authority")

prepare = (root / "scripts/prepare_release_target_reference.sh").read_text(encoding="utf-8")
for marker in [
    'resolve_android_emulator_revision',
    '"$sdk_root/emulator/source.properties"',
    'emulator-package=$EMULATOR_REVISION',
    'Docker image has no immutable repository digest',
    '$image.RepoDigests | sort | join(",")',
]:
    if marker not in prepare:
        raise SystemExit(f"test-execution-profile-matrix: reference preparation hardening missing: {marker}")

measurement = (root / "scripts/lib/release_full_measurement.py").read_text(encoding="utf-8")
for marker in [
    'KIND = "genesis/release-full-measurement-v0.1"',
    'PAIR_KIND = "genesis/release-full-measurement-pair-v0.1"',
    "MIN_PAIRS = 2",
    "WALL_BUDGET_MS = 2_700_000",
    "SESSION_BUDGET_MS = 3_000_000",
    "bounded child stderr tail",
    "min(WALL_BUDGET_MS, remaining_ms)",
    "ARTIFACT_BUDGET_BYTES = 20 * 1024 * 1024 * 1024",
    'for run_class in ("cold", "warm")',
    "process-tree peak RSS sampling produced no measurement",
    "owned-ephemeral-root-removal",
    "job-unique-external-owned-root",
    "containment.mkdir(mode=0o700, parents=False, exist_ok=False)",
    "cache_isolation_record(pair_index, github, cache_nonce)",
    "measurement pair workers reused a cache-isolation identity",
    "measurement aggregate pair coverage is incomplete or duplicated",
    "pair workers and target readiness do not share one workflow run attempt",
    "expected unsupported-product was relabeled as release qualification",
    "named target report policy or product binding mismatch",
]:
    if marker not in measurement:
        raise SystemExit(f"test-execution-profile-matrix: release measurement marker missing: {marker}")

release_execution = (root / "scripts/lib/release_evidence_execution.py").read_text(encoding="utf-8")
release_fanout = (root / "scripts/lib/release_evidence_fanout.py").read_text(encoding="utf-8")
for marker in [
    'WORKER_KIND = "genesis/release-evidence-worker-observation-v0.2"',
    'AGGREGATE_KIND = "genesis/release-evidence-aggregate-v0.2"',
    '"commandCoverageExact"',
    '"exclusive-owned-ephemeral-root"',
    "dependency_mirror.network_guard_prefix(allow_loopback=True)",
    "dependency_mirror.prove_network_denial(prefix, require_loopback=True)",
    '"measured": False',
    "genesis/release-evidence-worker-start-v0.2",
    "require_initialized_output",
    "release aggregate has a missing or duplicate execution node",
    "fanout consumer does not bind the cold-1 producer",
    "worker cleanup is incomplete",
    '"hostHandleLifecycle": host_handle_lifecycle',
    "retain_host_handle_lifecycle_evidence",
    "validate_host_handle_lifecycle_custody",
    'custody/host_bridge_fault_injection_report.json',
]:
    if marker not in release_execution:
        raise SystemExit(f"test-execution-profile-matrix: v0.2 execution marker missing: {marker}")
host_lifecycle = (root / "scripts/lib/host_handle_lifecycle_evidence.py").read_text(encoding="utf-8")
for marker in [
    'LINUX_ARTIFACT = "host-handle-lifecycle/linux/host_bridge_fault_injection_report.json"',
    'MACOS_ARTIFACT = "host-handle-lifecycle/macos/host_bridge_daemon_lifecycle_report.json"',
    '"r2_2_f_closeable": True',
    '"independentCrossHostEvidence": True',
    "tier-1 lifecycle reports do not share probe and self-host identities",
    "lifecycle evidence negative-control inventory drifted",
]:
    if marker not in host_lifecycle:
        raise SystemExit(f"test-execution-profile-matrix: host lifecycle reconciler marker missing: {marker}")
for marker in [
    'KIND = "genesis/release-evidence-fanout-auth-v0.2"',
    "same-run cold-1 fanout artifact did not become available before deadline",
    "fanout archive path is unsafe or duplicated",
    "fanout authentication is from another workflow run, attempt, or revision",
    "downloaded fanout archive digest does not match GitHub",
]:
    if marker not in release_fanout:
        raise SystemExit(f"test-execution-profile-matrix: v0.2 fanout marker missing: {marker}")

ai_slo = (root / "scripts/render_ai_iteration_slo_report.sh").read_text(encoding="utf-8")
for marker in [
    "CHANGED_FAST_SAMPLE_CEILING_MS=120000",
    'SAMPLES_CHANGED_FAST="${GENESIS_AI_ITERATION_SLO_SAMPLES_CHANGED_FAST:-3}"',
    'SAMPLES_CORE_SUITE="${GENESIS_AI_ITERATION_SLO_SAMPLES_CORE_SUITE:-3}"',
    'SAMPLES_GCPM_LOCK="${GENESIS_AI_ITERATION_SLO_SAMPLES_GCPM_LOCK:-3}"',
    'SAMPLES_GCPM_ENV="${GENESIS_AI_ITERATION_SLO_SAMPLES_GCPM_ENV:-3}"',
    "require_robust_median_sample_count",
    '--budget-ms "$CHANGED_FAST_SAMPLE_CEILING_MS"',
    '[[ "$CHANGED_FAST_MS" -le "$BUDGET_CHANGED_FAST_MS" ]]',
    '"changed_fast_sample_ceiling_ms": int(changed_fast_sample_ceiling_ms_s)',
]:
    if marker not in ai_slo:
        raise SystemExit(f"test-execution-profile-matrix: AI SLO sample contract missing: {marker}")
if '--budget-ms "$BUDGET_CHANGED_FAST_MS"' in ai_slo:
    raise SystemExit("test-execution-profile-matrix: changed-fast samples still enforce the median SLO per run")

def integer_median(values):
    ordered = sorted(values)
    midpoint = len(ordered) // 2
    if len(ordered) % 2:
        return ordered[midpoint]
    return int((ordered[midpoint - 1] + ordered[midpoint]) / 2.0)

if integer_median([3_548, 3_600, 16_367]) > 15_000:
    raise SystemExit("test-execution-profile-matrix: one collectable outlier corrupted median adjudication")
if integer_median([16_001, 16_001, 16_001]) <= 15_000:
    raise SystemExit("test-execution-profile-matrix: over-budget median was accepted")

consumers = {
    "scripts/check_agent_generative_workloads.sh",
    "scripts/check_agent_scenario_perf.sh",
    "scripts/check_agent_workflow_runtime_parity.sh",
    "scripts/check_gpu_xr_productization_kits.sh",
    "scripts/check_host_bridge_fault_injection.sh",
    "scripts/check_runtime_backend_feature_matrix.sh",
    "scripts/check_slo_report_contracts.sh",
    "scripts/check_write_genesiscode_skill_conformance.sh",
    "scripts/check_write_genesiscode_skill_distribution.sh",
}
for relative in sorted(consumers):
    source = (root / relative).read_text(encoding="utf-8")
    if "genesis_verify_health_profile_evidence" not in source:
        raise SystemExit(f"test-execution-profile-matrix: evidence consumer bypasses verifier: {relative}")
PY

PYTHONPATH="$ROOT_DIR/scripts/lib" python3 - "$ROOT_DIR" <<'PY'
from pathlib import Path
import copy
import json
import os
import sys
import tempfile

import cargo_cache
import release_full_measurement as measurement

root = Path(sys.argv[1]).resolve()
with tempfile.TemporaryDirectory(prefix="genesis-release-cache-isolation.") as raw:
    temp = Path(raw)
    inherited = dict(os.environ)
    inherited["GENESIS_CARGO_CACHE_ROOT"] = str(temp / "parent-cache")
    resolved = cargo_cache.resolve(root, "root-host", inherited)
    key = resolved["metadata"]["cacheKeySha256"]
    inherited_target = Path(resolved["target_dir"])
    inherited_target.mkdir(parents=True)
    (inherited_target / resolved["metadata_file"]).write_bytes(
        cargo_cache.pretty_bytes(resolved["metadata"])
    )
    inherited.update({
        "CARGO_TARGET_DIR": str(inherited_target),
        "GENESIS_CARGO_CACHE_RESOLVED": "1",
        "GENESIS_CARGO_CACHE_SCOPE": "root-host",
        "GENESIS_CARGO_CACHE_KEY_SHA256": key,
        "GENESIS_CARGO_CACHE_HIT": "1",
        "GENESIS_CARGO_CACHE_ROOT": str(temp / "parent-cache"),
        "GENESIS_GENERATED_STATE_ROOT": str(root),
        "GENESIS_GENERATED_STATE_LEASE_PID": "123",
        "GENESIS_GENERATED_STATE_LEASE_TOKEN": "fixture-token",
        "UNRELATED": "preserved",
    })
    pair_cache = temp / "pair-cache"
    child = measurement.measurement_environment(root, pair_cache, inherited)
    if child.get("GENESIS_CARGO_CACHE_ROOT") != str(pair_cache):
        raise SystemExit("test-execution-profile-matrix: pair-owned cache root was not selected")
    if child.get("UNRELATED") != "preserved":
        raise SystemExit("test-execution-profile-matrix: unrelated environment was not preserved")
    leaked = sorted((measurement.CARGO_CACHE_ENV - {"GENESIS_CARGO_CACHE_ROOT"}) & child.keys())
    if leaked:
        raise SystemExit(f"test-execution-profile-matrix: inherited cache provenance leaked: {leaked}")

    negative_controls = 0
    cases = [
        ({"CARGO_TARGET_DIR": str(temp / "arbitrary")}, "arbitrary inherited CARGO_TARGET_DIR"),
        ({"GENESIS_CARGO_CACHE_RESOLVED": "1"}, "missing CARGO_TARGET_DIR"),
        (
            {
                "CARGO_TARGET_DIR": str(inherited_target),
                "GENESIS_CARGO_CACHE_RESOLVED": "1",
                "GENESIS_CARGO_CACHE_SCOPE": "root-host",
            },
            "provenance is incomplete",
        ),
    ]
    for environ, expected in cases:
        try:
            measurement.measurement_environment(root, pair_cache, environ)
        except measurement.MeasurementError as exc:
            if expected not in str(exc):
                raise SystemExit(
                    f"test-execution-profile-matrix: wrong cache isolation failure: {exc}"
                )
        else:
            raise SystemExit(
                f"test-execution-profile-matrix: cache isolation accepted invalid state: {expected}"
            )
        negative_controls += 1

    mismatched = dict(inherited)
    mismatched["CARGO_TARGET_DIR"] = str(temp / "wrong-target")
    try:
        measurement.measurement_environment(root, pair_cache, mismatched)
    except measurement.MeasurementError as exc:
        if "does not match the canonical resolver" not in str(exc):
            raise SystemExit(f"test-execution-profile-matrix: wrong mismatch failure: {exc}")
    else:
        raise SystemExit("test-execution-profile-matrix: mismatched inherited target was accepted")
    negative_controls += 1

    (inherited_target / resolved["metadata_file"]).write_text("{}\n", encoding="utf-8")
    try:
        measurement.measurement_environment(root, pair_cache, inherited)
    except measurement.MeasurementError as exc:
        if "metadata does not match" not in str(exc):
            raise SystemExit(f"test-execution-profile-matrix: wrong metadata failure: {exc}")
    else:
        raise SystemExit("test-execution-profile-matrix: tampered cache metadata was accepted")
    negative_controls += 1

    diagnostic_path = temp / "diagnostic.log"
    diagnostic_path.write_text(
        "\n".join(
            [f"{root}/private/line-{index}\x1b[31m" for index in range(100)]
            + ["final portable failure"]
        ),
        encoding="utf-8",
    )
    tail = measurement.diagnostic_tail(diagnostic_path, root)
    if len(tail.encode("utf-8")) > measurement.DIAGNOSTIC_TAIL_MAX_BYTES:
        raise SystemExit("test-execution-profile-matrix: diagnostic tail exceeded byte bound")
    if len(tail.splitlines()) > measurement.DIAGNOSTIC_TAIL_MAX_LINES:
        raise SystemExit("test-execution-profile-matrix: diagnostic tail exceeded line bound")
    if str(root) in tail or "\x1b" in tail or "final portable failure" not in tail:
        raise SystemExit("test-execution-profile-matrix: diagnostic tail was not portable and terminal")
    negative_controls += 1

    source = {"gitCommit": "a" * 40}
    github = {
        "job": "release_full_measurement_pair",
        "runAttempt": "1",
        "runId": "42",
        "sha": source["gitCommit"],
    }

    def run_row(pair, run_class):
        prefix = f"runs/pair-{pair:02d}-{run_class}"
        return {
            "agentGpuProfile": "agent-gpu-fallback",
            "artifactAttributionBytes": {
                "isolated-run": 1,
                "node-modules": 0,
                "workspace-build": 0,
                "workspace-target": 0,
            },
            "artifactPeakBytes": 1,
            "cacheRootStartedEmpty": run_class == "cold",
            "class": run_class,
            "exitCode": 0,
            "index": pair,
            "logArtifacts": [f"{prefix}/stdout.log", f"{prefix}/stderr.log"],
            "peakRssBytes": 1,
            "profileElapsedMs": 1,
            "profileReportArtifact": f"{prefix}/profile-report.json",
            "profileReportSha256": "b" * 64,
            "telemetryElapsedMs": 2,
        }

    target_rows = [
        {
            "expectedOutcome": "unsupported-product",
            "githubRunAttempt": "1",
            "githubRunId": "42",
            "githubSha": source["gitCommit"],
            "releaseQualified": False,
            "reportArtifact": f"target-readiness/{target}.json",
            "reportSha256": "c" * 64,
            "runner": measurement.TARGET_RUNNERS[target],
            "target": target,
        }
        for target in measurement.TARGETS
    ]
    pair_reports = []
    for pair in (1, 2):
        pair_report = {
            "artifacts": [],
            "budgets": {
                "maxArtifactBytes": measurement.ARTIFACT_BUDGET_BYTES,
                "maxPairSessionMs": measurement.SESSION_BUDGET_MS,
                "maxWallMs": measurement.WALL_BUDGET_MS,
            },
            "cacheIsolation": measurement.cache_isolation_record(
                pair,
                github,
                nonce_sha256=f"{pair}" * 64,
            ),
            "cleanupRecovery": {
                "method": "owned-ephemeral-root-removal",
                "ok": True,
                "pair": pair,
                "recoveredBytes": 1,
                "remainingBytes": 0,
            },
            "contentIdentitySha256": "",
            "executionEnvironment": {},
            "generatedAtUtc": "2026-08-05T00:00:00+00:00",
            "github": github,
            "kind": measurement.PAIR_KIND,
            "ok": True,
            "pair": pair,
            "runs": [run_row(pair, "cold"), run_row(pair, "warm")],
            "source": source,
            "version": measurement.PAIR_VERSION,
        }
        pair_report["contentIdentitySha256"] = measurement.identity(pair_report)
        measurement.validate_pair_report(pair_report, pair)
        pair_reports.append(pair_report)

    pair_dirs = []
    for report in pair_reports:
        pair_dir = temp / f"pair-{report['pair']}"
        pair_dir.mkdir()
        (pair_dir / "manifest.json").write_text(
            json.dumps(report, sort_keys=True) + "\n",
            encoding="utf-8",
        )
        pair_dirs.append(pair_dir)
    target_paths = []
    for target in measurement.TARGETS:
        path = temp / f"target-{target}.json"
        path.write_text(json.dumps({"targets": [{"target": target}]}) + "\n", encoding="utf-8")
        target_paths.append(path)
    originals = (
        measurement.verify_pair,
        measurement.validate_target_reports,
        measurement.health_evidence.source_inventory,
        measurement.health_evidence.execution_environment,
        measurement.verify,
    )
    try:
        by_pair = {report["pair"]: report for report in pair_reports}
        measurement.verify_pair = lambda _root, path, _expected=None: by_pair[int(path.name.rsplit("-", 1)[1])]
        measurement.validate_target_reports = lambda _root, _paths: target_rows
        measurement.health_evidence.source_inventory = lambda _root: source
        measurement.health_evidence.execution_environment = lambda _profile: {}
        measurement.verify = lambda _root, output: measurement.validate_report(
            measurement.load_json(output / "manifest.json")
        )
        measurement.aggregate(root, temp / "aggregate-smoke", target_paths, pair_dirs)
    finally:
        (
            measurement.verify_pair,
            measurement.validate_target_reports,
            measurement.health_evidence.source_inventory,
            measurement.health_evidence.execution_environment,
            measurement.verify,
        ) = originals

    pair_mutations = [
        (lambda d: d["github"].__setitem__("job", "untrusted"), "GitHub job identity mismatch"),
        (lambda d: d["cacheIsolation"].__setitem__("identitySha256", "0" * 64), "cache-isolation identity mismatch"),
    ]
    for mutate, expected in pair_mutations:
        invalid = copy.deepcopy(pair_reports[0])
        mutate(invalid)
        invalid["contentIdentitySha256"] = measurement.identity(invalid)
        try:
            measurement.validate_pair_report(invalid, 1)
        except measurement.MeasurementError as exc:
            if expected not in str(exc):
                raise SystemExit(f"test-execution-profile-matrix: wrong pair failure: {exc}")
        else:
            raise SystemExit(f"test-execution-profile-matrix: invalid pair evidence accepted: {expected}")
        negative_controls += 1

    execution_sha = measurement.sha256_bytes(measurement.canonical({}))
    pair_workers = []
    for report in pair_reports:
        pair = report["pair"]
        pair_workers.append({
            "cacheIsolationIdentitySha256": report["cacheIsolation"]["identitySha256"],
            "executionEnvironmentSha256": execution_sha,
            "githubJob": github["job"],
            "githubRunAttempt": github["runAttempt"],
            "githubRunId": github["runId"],
            "githubSha": github["sha"],
            "pair": pair,
            "workerContentIdentitySha256": report["contentIdentitySha256"],
            "workerManifestArtifact": f"workers/pair-{pair:02d}.json",
            "workerManifestSha256": "d" * 64,
        })
    aggregate = {
        "artifacts": [],
        "budgets": {
            "maxArtifactBytes": measurement.ARTIFACT_BUDGET_BYTES,
            "maxWallMs": measurement.WALL_BUDGET_MS,
            "minimumPairs": measurement.MIN_PAIRS,
        },
        "cleanupRecovery": [report["cleanupRecovery"] for report in pair_reports],
        "contentIdentitySha256": "",
        "executionEnvironment": {},
        "generatedAtUtc": "2026-08-05T00:00:00+00:00",
        "history": {
            "coldP95ArtifactBytes": 1,
            "coldP95PeakRssBytes": 1,
            "coldP95WallMs": 2,
            "samplesPerClass": 2,
            "warmP95ArtifactBytes": 1,
            "warmP95PeakRssBytes": 1,
            "warmP95WallMs": 2,
        },
        "kind": measurement.KIND,
        "ok": True,
        "pairWorkers": pair_workers,
        "pairs": 2,
        "productReleaseQualified": False,
        "profileOperational": True,
        "readinessStatus": "unsupported-product",
        "runs": [row for report in pair_reports for row in report["runs"]],
        "source": source,
        "targetReadiness": target_rows,
        "version": measurement.VERSION,
    }
    aggregate["contentIdentitySha256"] = measurement.identity(aggregate)
    measurement.validate_report(aggregate)

    aggregate_mutations = [
        (lambda d: d["pairWorkers"][1].__setitem__("cacheIsolationIdentitySha256", d["pairWorkers"][0]["cacheIsolationIdentitySha256"]), "reused a cache-isolation identity"),
        (lambda d: d.__setitem__("pairWorkers", d["pairWorkers"][:-1]), "worker set is incomplete"),
        (lambda d: d["pairWorkers"][1].__setitem__("githubRunAttempt", "2"), "do not share one workflow run attempt"),
        (lambda d: d["pairWorkers"][1].__setitem__("workerManifestArtifact", "workers/pair-01.json"), "worker identity mismatch"),
    ]
    for mutate, expected in aggregate_mutations:
        invalid = copy.deepcopy(aggregate)
        mutate(invalid)
        invalid["contentIdentitySha256"] = measurement.identity(invalid)
        try:
            measurement.validate_report(invalid)
        except measurement.MeasurementError as exc:
            if expected not in str(exc):
                raise SystemExit(f"test-execution-profile-matrix: wrong aggregate failure: {exc}")
        else:
            raise SystemExit(f"test-execution-profile-matrix: invalid aggregate accepted: {expected}")
        negative_controls += 1

print(f"release-full-measurement-cache-isolation: ok (negative_controls={negative_controls})")
PY

WRITE_SKILL_FIXTURE="$(mktemp -d)"
cleanup_write_skill_fixture() {
  rm -rf "$WRITE_SKILL_FIXTURE"
}
trap cleanup_write_skill_fixture EXIT INT TERM
mkdir -p \
  "$WRITE_SKILL_FIXTURE/evidence" \
  "$WRITE_SKILL_FIXTURE/kit"
cp docs/skill_pack/write_genesiscode_v1/manifest.json "$WRITE_SKILL_FIXTURE/kit/manifest.json"
cp docs/skill_pack/write_genesiscode_v1/prompt-cards.json "$WRITE_SKILL_FIXTURE/kit/prompt-cards.json"
cp docs/skill_pack/write_genesiscode_v1/recipe-cards.json "$WRITE_SKILL_FIXTURE/kit/recipe-cards.json"
python3 - "$WRITE_SKILL_FIXTURE/kit/manifest.json" "$WRITE_SKILL_FIXTURE/missing-retained-report.json" <<'PY'
import json
from pathlib import Path
import sys

path = Path(sys.argv[1])
document = json.loads(path.read_text(encoding="utf-8"))
document["expected_reports"][0]["path"] = sys.argv[2]
path.write_text(json.dumps(document, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY
PYTHONPATH="$ROOT_DIR/scripts/lib" python3 - "$ROOT_DIR" "$WRITE_SKILL_FIXTURE/evidence" <<'PY'
import json
from pathlib import Path
import sys

import health_profile_evidence as evidence

root = Path(sys.argv[1]).resolve()
output = Path(sys.argv[2]).resolve()
for name, (kind, _) in evidence.ARTIFACTS.items():
    path = output / name
    if kind == "jsonl-history":
        path.write_text('{"fixture":true}\n', encoding="utf-8")
    else:
        path.write_text(
            json.dumps({"kind": kind, "ok": True}, sort_keys=True) + "\n",
            encoding="utf-8",
        )
manifest = evidence.build(root, output, "release-full")
(output / "manifest.json").write_text(
    json.dumps(manifest, indent=2, sort_keys=True) + "\n",
    encoding="utf-8",
)
PY
cat >"$WRITE_SKILL_FIXTURE/bash-env" <<'SH'
bash() {
  [[ "${1:-}" == "scripts/check_write_genesiscode_skill_conformance.sh" ]]
  local binding
  for binding in \
    GENESIS_HEALTH_EVIDENCE_MANIFEST \
    GENESIS_WRITE_SKILL_GAUNTLET_REPORT \
    GENESIS_WRITE_SKILL_GENERATIVE_REPORT \
    GENESIS_WRITE_SKILL_RUNTIME_BACKEND_REPORT \
    GENESIS_WRITE_SKILL_HOST_BRIDGE_REPORT \
    GENESIS_WRITE_SKILL_GPU_XR_REPORT \
    GENESIS_WRITE_SKILL_ASSURANCE_REPORT; do
    [[ -n "${!binding:-}" ]]
  done
  : >"${GENESIS_WRITE_SKILL_FIXTURE_CALLED:?}"
}
SH

WRITE_SKILL_BASE_ENV=(
  "BASH_ENV=$WRITE_SKILL_FIXTURE/bash-env"
  "GENESIS_GATE_TELEMETRY_DISABLE=1"
  "GENESIS_WRITE_SKILL_DIST_ROOT=$WRITE_SKILL_FIXTURE/kit"
  "GENESIS_WRITE_SKILL_DIST_MANIFEST=$WRITE_SKILL_FIXTURE/kit/manifest.json"
  "GENESIS_WRITE_SKILL_DIST_VERIFY_RUNTIME=1"
  "GENESIS_WRITE_SKILL_CONFORMANCE_PROFILE=release-full"
  "GENESIS_WRITE_SKILL_FIXTURE_CALLED=$WRITE_SKILL_FIXTURE/conformance-called"
)
WRITE_SKILL_EVIDENCE_ENV=(
  "GENESIS_HEALTH_EVIDENCE_REQUIRED=1"
  "GENESIS_HEALTH_EVIDENCE_MANIFEST=$WRITE_SKILL_FIXTURE/evidence/manifest.json"
  "GENESIS_WRITE_SKILL_GAUNTLET_REPORT=$WRITE_SKILL_FIXTURE/evidence/agent_capability_gauntlet_report.json"
  "GENESIS_WRITE_SKILL_GENERATIVE_REPORT=$WRITE_SKILL_FIXTURE/evidence/agent_generative_workloads_report.json"
  "GENESIS_WRITE_SKILL_RUNTIME_BACKEND_REPORT=$WRITE_SKILL_FIXTURE/evidence/runtime_backend_feature_matrix_report.json"
  "GENESIS_WRITE_SKILL_HOST_BRIDGE_REPORT=$WRITE_SKILL_FIXTURE/evidence/host_bridge_fault_injection_report.json"
  "GENESIS_WRITE_SKILL_GPU_XR_REPORT=$WRITE_SKILL_FIXTURE/evidence/gpu_xr_productization_kits_report.json"
  "GENESIS_WRITE_SKILL_ASSURANCE_REPORT=$WRITE_SKILL_FIXTURE/evidence/assurance_profile_packs_report.json"
)
write_skill_negative_controls=0
if env "${WRITE_SKILL_BASE_ENV[@]}" \
  /bin/bash "$WRITE_SKILL_DISTRIBUTION_SCRIPT" >/dev/null 2>&1; then
  echo "test-execution-profile-matrix: release distribution accepted absent private evidence" >&2
  exit 1
fi
write_skill_negative_controls=$((write_skill_negative_controls + 1))
if env "${WRITE_SKILL_BASE_ENV[@]}" \
  "${WRITE_SKILL_EVIDENCE_ENV[@]:0:7}" \
  /bin/bash "$WRITE_SKILL_DISTRIBUTION_SCRIPT" >/dev/null 2>&1; then
  echo "test-execution-profile-matrix: release distribution accepted a missing report binding" >&2
  exit 1
fi
write_skill_negative_controls=$((write_skill_negative_controls + 1))
env "${WRITE_SKILL_BASE_ENV[@]}" "${WRITE_SKILL_EVIDENCE_ENV[@]}" \
  /bin/bash "$WRITE_SKILL_DISTRIBUTION_SCRIPT" >/dev/null
[[ -f "$WRITE_SKILL_FIXTURE/conformance-called" ]] || {
  echo "test-execution-profile-matrix: release distribution did not invoke conformance" >&2
  exit 1
}
printf '{"kind":"tampered","ok":true}\n' \
  >"$WRITE_SKILL_FIXTURE/evidence/assurance_profile_packs_report.json"
if env "${WRITE_SKILL_BASE_ENV[@]}" "${WRITE_SKILL_EVIDENCE_ENV[@]}" \
  /bin/bash "$WRITE_SKILL_DISTRIBUTION_SCRIPT" >/dev/null 2>&1; then
  echo "test-execution-profile-matrix: release distribution accepted tampered evidence" >&2
  exit 1
fi
write_skill_negative_controls=$((write_skill_negative_controls + 1))
cleanup_write_skill_fixture
trap - EXIT INT TERM
echo "release-write-skill-evidence-binding: ok (negative_controls=$write_skill_negative_controls)"

HEALTH_OUTPUT_FIXTURE="$(mktemp -d)"
cleanup_health_output_fixture() {
  rm -rf "$HEALTH_OUTPUT_FIXTURE"
}
trap cleanup_health_output_fixture EXIT INT TERM
mkdir "$HEALTH_OUTPUT_FIXTURE/retained"
printf '%s\n' 'parent-owned' > "$HEALTH_OUTPUT_FIXTURE/retained/parent.txt"
GENESIS_CHECK_HEALTH_OUTPUT_CONTAINMENT_ROOT="$HEALTH_OUTPUT_FIXTURE" \
GENESIS_CHECK_HEALTH_OUTPUT_ROOT="$HEALTH_OUTPUT_FIXTURE/retained" \
  bash scripts/check_check_update_boundary.sh >/dev/null
if [[ "$(find "$HEALTH_OUTPUT_FIXTURE/retained" -mindepth 1 -maxdepth 1 -print)" != \
      "$HEALTH_OUTPUT_FIXTURE/retained/parent.txt" ]]; then
  echo "test-execution-profile-matrix: nested health canary mutated parent private output" >&2
  exit 1
fi
cleanup_health_output_fixture
trap - EXIT INT TERM
echo "nested-health-output-isolation: ok"

bash scripts/test_prepare_release_target_reference.sh

python3 scripts/lib/ci_runner_preflight.py self-test \
  --policy policies/ci_control_plane_v0.1.json
python3 scripts/lib/ci_liveness_watchdog.py self-test \
  --policy policies/ci_control_plane_v0.1.json
python3 "$RELEASE_EVIDENCE_FANOUT_RUNNER" self-test

python3 "$LINT_SUPPRESSION_POLICY" --root "$ROOT_DIR"

echo "test-execution-profile-matrix: ok"
