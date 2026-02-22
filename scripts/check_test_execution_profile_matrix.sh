#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

DOC="docs/spec/TEST_EXECUTION_PROFILES_v0.1.md"
CI=".github/workflows/ci.yml"
CHANGED_FAST_SCRIPT="scripts/test_changed_fast.sh"
DEFAULT_LOOP_SCRIPT="scripts/check_default_iteration_workflow.sh"
STRICT_GOLDEN_SCRIPT="scripts/selfhost_strict_golden.sh"
WASM_CROSS_HOST_SCRIPT="scripts/wasm_cross_host_determinism.mjs"
FULL_CROSS_HOST_BUDGET_SCRIPT="scripts/check_full_cross_host_profile_budget.sh"
AGENT_GENERATIVE_SCRIPT="scripts/check_agent_generative_workloads.sh"

for path in \
  "$DOC" \
  "$CI" \
  "$CHANGED_FAST_SCRIPT" \
  "$DEFAULT_LOOP_SCRIPT" \
  "$STRICT_GOLDEN_SCRIPT" \
  "$WASM_CROSS_HOST_SCRIPT" \
  "$FULL_CROSS_HOST_BUDGET_SCRIPT" \
  "$AGENT_GENERATIVE_SCRIPT"; do
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
require_doc_pattern '| `strict-golden` |'
require_doc_pattern '| `full-cross-host` |'
require_doc_pattern '`<= 2m`'
require_doc_pattern '`<= 3m`'
require_doc_pattern '`<= 8m`'
require_doc_pattern '`<= 12m`'
require_doc_pattern 'scripts/check_upgrade_plan_health.sh --profile prepush-standard'
require_doc_pattern 'genesis/upgrade-plan-health-profile-v0.1'
require_doc_pattern 'GENESIS_HEALTH_PREPUSH_BUDGET_MS'
require_doc_pattern 'GENESIS_HEALTH_SHARDS'
require_doc_pattern 'GENESIS_HEALTH_CARGO_TARGET_DIR'
require_doc_pattern 'GENESIS_HEALTH_CARGO_GATE_SHARDS'
require_doc_pattern 'GENESIS_HEALTH_WARM_CARGO_CACHE=auto|1|0'
require_doc_pattern 'genesis/upgrade-plan-health-cargo-warmup-v0.1'
require_doc_pattern 'release-full` profile requires `scripts/check_gpu_compute_device_conformance.sh` by default'
require_doc_pattern 'AI Iteration SLO Contention Policy'
require_doc_pattern 'median-of-samples'
require_doc_pattern 'GENESIS_AI_ITERATION_SLO_SAMPLES_INCREMENTAL_WARM'
require_doc_pattern 'GENESIS_AI_ITERATION_SLO_CONTENTION_WARN_PERCENT'
require_doc_pattern 'GENESIS_AI_ITERATION_SLO_WARMUP_GCPM_LOCK'
require_doc_pattern 'GENESIS_AI_ITERATION_SLO_STABILIZE_RETRIES_GCPM_LOCK'
require_doc_pattern '.genesis/perf/strict_golden_profile_report.json'
require_doc_pattern '.genesis/perf/wasm_cross_host_profile_report.json'
require_doc_pattern '.genesis/perf/full_cross_host_profile_report.json'
require_doc_pattern '.genesis/perf/agent_scenario_perf_report.json'
require_doc_pattern '.genesis/perf/agent_generative_workloads_report.json'
require_doc_pattern 'policies/perf/full_cross_host_profile_seed_history.jsonl'
require_doc_pattern 'policies/perf/agent_scenario_perf_seed_history.jsonl'
require_doc_pattern 'scripts/check_full_cross_host_profile_budget.sh'
require_doc_pattern 'scripts/check_agent_scenario_perf.sh'
require_doc_pattern 'scripts/check_agent_generative_workloads.sh'

require_ci_pattern 'Changed-File Fast Loop Budget'
require_ci_pattern 'Selfhost Refactor Guard'
require_ci_pattern 'Selfhost Strict Smoke (Native + WASI CLI)'
require_ci_pattern 'Selfhost Strict Golden (Native + WASI CLI)'
require_ci_pattern 'WASM Cross-Host Determinism (Native vs Node)'
require_ci_pattern 'Full Cross-Host Runtime Budget Gate'
require_ci_pattern 'Full Cross-Host Runtime Budget Gate (PR Required)'
require_ci_pattern 'Agent End-to-End Scenario Perf Gate'
require_ci_pattern 'Agent Generative Workload Gate'

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

if ! grep -Fq 'GENESIS_FULL_CROSS_HOST_BUDGET_MS:-720000' "$FULL_CROSS_HOST_BUDGET_SCRIPT"; then
  echo "test-execution-profile-matrix: full-cross-host default budget must remain 720000ms (12m)" >&2
  exit 1
fi

if ! grep -Fq 'GENESIS_HEALTH_PREPUSH_BUDGET_MS:-240000' scripts/check_upgrade_plan_health.sh; then
  echo "test-execution-profile-matrix: prepush strict loop budget must remain pinned at default 240000ms (4m)" >&2
  exit 1
fi

if ! grep -Fq 'default_health_shards_for_profile' scripts/check_upgrade_plan_health.sh; then
  echo "test-execution-profile-matrix: check_upgrade_plan_health.sh must keep deterministic shard default function" >&2
  exit 1
fi

if ! grep -Fq 'PROFILE_SHARDS' scripts/check_upgrade_plan_health.sh; then
  echo "test-execution-profile-matrix: check_upgrade_plan_health.sh must keep dedicated profile shard control" >&2
  exit 1
fi

if ! grep -Fq 'GENESIS_HEALTH_CARGO_GATE_SHARDS' scripts/check_upgrade_plan_health.sh; then
  echo "test-execution-profile-matrix: check_upgrade_plan_health.sh must keep dedicated cargo gate shard control" >&2
  exit 1
fi

if ! grep -Fq 'export CARGO_TARGET_DIR="$HEALTH_CARGO_TARGET_DIR"' scripts/check_upgrade_plan_health.sh; then
  echo "test-execution-profile-matrix: check_upgrade_plan_health.sh must export shared CARGO_TARGET_DIR for health gates" >&2
  exit 1
fi

if ! grep -Fq 'partition_gate_commands' scripts/check_upgrade_plan_health.sh; then
  echo "test-execution-profile-matrix: check_upgrade_plan_health.sh must partition cargo vs non-cargo gate scheduling" >&2
  exit 1
fi

if ! grep -Fq 'GENESIS_HEALTH_REQUIRE_GPU_DEVICE_CONFORMANCE' scripts/check_upgrade_plan_health.sh; then
  echo "test-execution-profile-matrix: check_upgrade_plan_health.sh must keep explicit gpu device conformance lane toggle" >&2
  exit 1
fi

if ! grep -Fq 'bash scripts/check_agent_scenario_perf.sh' scripts/check_upgrade_plan_health.sh; then
  echo "test-execution-profile-matrix: release-full profile must run agent scenario perf gate" >&2
  exit 1
fi

if ! grep -Fq 'if [[ "$PROFILE" == "release-full" ]]; then' scripts/check_upgrade_plan_health.sh || \
   ! grep -Fq 'GPU_DEVICE_CONFORMANCE="1"' scripts/check_upgrade_plan_health.sh; then
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

if ! grep -Fq 'full-cross-host' "$FULL_CROSS_HOST_BUDGET_SCRIPT"; then
  echo "test-execution-profile-matrix: full cross-host budget script must stamp full-cross-host profile label into runtime report" >&2
  exit 1
fi

if ! grep -Fq 'profile_runtime_budget.py' "$FULL_CROSS_HOST_BUDGET_SCRIPT"; then
  echo "test-execution-profile-matrix: full cross-host budget script must emit/enforce aggregate runtime report via shared profile budget helper" >&2
  exit 1
fi

if ! grep -Fq 'GENESIS_FULL_CROSS_HOST_BASELINE_HISTORY' "$FULL_CROSS_HOST_BUDGET_SCRIPT"; then
  echo "test-execution-profile-matrix: full cross-host budget script must expose baseline seed history path" >&2
  exit 1
fi

if ! grep -Fq -- '--baseline-history "$FULL_BASELINE_HISTORY"' "$FULL_CROSS_HOST_BUDGET_SCRIPT"; then
  echo "test-execution-profile-matrix: full cross-host budget script must pass baseline history to shared runtime budget helper" >&2
  exit 1
fi

if ! grep -Fq -- '--require-min-history' "$FULL_CROSS_HOST_BUDGET_SCRIPT"; then
  echo "test-execution-profile-matrix: full cross-host budget script must fail-closed on insufficient history depth" >&2
  exit 1
fi

if ! grep -Fq 'GENESIS_AGENT_SCENARIO_BASELINE_HISTORY' scripts/check_agent_scenario_perf.sh; then
  echo "test-execution-profile-matrix: agent scenario perf gate must expose baseline seed history path" >&2
  exit 1
fi

if ! grep -Fq 'GENESIS_AGENT_SCENARIO_REQUIRE_MIN_HISTORY' scripts/check_agent_scenario_perf.sh; then
  echo "test-execution-profile-matrix: agent scenario perf gate must expose minimum-history fail-closed control" >&2
  exit 1
fi

if ! grep -Fq 'genesis/agent-generative-workloads-v0.1' "$AGENT_GENERATIVE_SCRIPT"; then
  echo "test-execution-profile-matrix: agent generative workload gate must emit stable report kind" >&2
  exit 1
fi

if ! grep -Fq 'GENESIS_AGENT_GENERATIVE_SECONDARY_REPORT' "$AGENT_GENERATIVE_SCRIPT"; then
  echo "test-execution-profile-matrix: agent generative workload gate must support secondary report parity mode" >&2
  exit 1
fi

echo "test-execution-profile-matrix: ok"
