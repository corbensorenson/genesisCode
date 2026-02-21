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

for path in \
  "$DOC" \
  "$CI" \
  "$CHANGED_FAST_SCRIPT" \
  "$DEFAULT_LOOP_SCRIPT" \
  "$STRICT_GOLDEN_SCRIPT" \
  "$WASM_CROSS_HOST_SCRIPT" \
  "$FULL_CROSS_HOST_BUDGET_SCRIPT"; do
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
require_doc_pattern '`<= 5m`'
require_doc_pattern '`<= 3m`'
require_doc_pattern '`<= 15m`'
require_doc_pattern '`<= 20m`'
require_doc_pattern 'scripts/check_upgrade_plan_health.sh --profile prepush-standard'
require_doc_pattern 'genesis/upgrade-plan-health-profile-v0.1'
require_doc_pattern 'GENESIS_HEALTH_PREPUSH_BUDGET_MS'
require_doc_pattern 'GENESIS_HEALTH_SHARDS'
require_doc_pattern 'release-full` profile requires `scripts/check_gpu_compute_device_conformance.sh` by default'
require_doc_pattern '.genesis/perf/strict_golden_profile_report.json'
require_doc_pattern '.genesis/perf/wasm_cross_host_profile_report.json'
require_doc_pattern '.genesis/perf/full_cross_host_profile_report.json'
require_doc_pattern 'scripts/check_full_cross_host_profile_budget.sh'

require_ci_pattern 'Changed-File Fast Loop Budget'
require_ci_pattern 'Selfhost Refactor Guard'
require_ci_pattern 'Selfhost Strict Smoke (Native + WASI CLI)'
require_ci_pattern 'Selfhost Strict Golden (Native + WASI CLI)'
require_ci_pattern 'WASM Cross-Host Determinism (Native vs Node)'
require_ci_pattern 'Full Cross-Host Runtime Budget Gate'
require_ci_pattern 'Full Cross-Host Runtime Budget Gate (PR Required)'

if ! grep -Fq 'GENESIS_TEST_CHANGED_BUDGET_MS:-300000' "$CHANGED_FAST_SCRIPT"; then
  echo "test-execution-profile-matrix: changed-fast default budget must remain 300000ms (5m)" >&2
  exit 1
fi

if ! grep -Fq 'GENESIS_BUDGET_CHANGED_FAST_MS:-300000' "$DEFAULT_LOOP_SCRIPT"; then
  echo "test-execution-profile-matrix: default iteration workflow budget must remain 300000ms (5m)" >&2
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

if ! grep -Fq 'GENESIS_HEALTH_REQUIRE_GPU_DEVICE_CONFORMANCE' scripts/check_upgrade_plan_health.sh; then
  echo "test-execution-profile-matrix: check_upgrade_plan_health.sh must keep explicit gpu device conformance lane toggle" >&2
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

echo "test-execution-profile-matrix: ok"
