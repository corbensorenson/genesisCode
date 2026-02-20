#!/usr/bin/env bash
set -euo pipefail

export LC_ALL=C

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

PLAN_FILE="upgrade_plan.md"
DEFAULT_PROFILE="dev-fast"
if [[ "${CI:-}" == "true" ]]; then
  DEFAULT_PROFILE="release-full"
fi
PROFILE="${GENESIS_HEALTH_PROFILE:-$DEFAULT_PROFILE}"
DEV_FAST_BUDGET_MS="${GENESIS_DEV_FAST_BUDGET_MS:-300000}"

usage() {
  cat <<'EOF'
Usage: scripts/check_upgrade_plan_health.sh [--profile <dev-fast|prepush-standard|release-full>]
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --profile)
      PROFILE="${2:-}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "upgrade-plan-health: unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [[ "$PROFILE" != "dev-fast" && "$PROFILE" != "prepush-standard" && "$PROFILE" != "release-full" ]]; then
  echo "upgrade-plan-health: invalid profile '$PROFILE' (expected dev-fast|prepush-standard|release-full)" >&2
  exit 2
fi

if [[ ! -f "$PLAN_FILE" ]]; then
  echo "upgrade-plan-health: missing file: $PLAN_FILE"
  exit 1
fi

declared_open="$(awk -F: '/^Open checklist items:/ { gsub(/[[:space:]]/, "", $2); print $2; exit }' "$PLAN_FILE")"
if [[ -z "$declared_open" || ! "$declared_open" =~ ^[0-9]+$ ]]; then
  echo "upgrade-plan-health: could not parse integer from 'Open checklist items:' line"
  exit 1
fi

actual_open="$( (grep -n '^- \[ \]' "$PLAN_FILE" || true) | wc -l | tr -d '[:space:]' )"
if [[ "$declared_open" != "$actual_open" ]]; then
  echo "upgrade-plan-health: declared open item count does not match unchecked checklist entries"
  echo "  declared_open=$declared_open"
  echo "  actual_open=$actual_open"
  exit 1
fi

if [[ "$declared_open" -gt 0 ]]; then
  echo "upgrade-plan-health: open checklist items = $declared_open (hard-gate sweep deferred until zero; profile=$PROFILE)"
  echo "upgrade-plan-health: ok"
  exit 0
fi

echo "upgrade-plan-health: open checklist items = 0; enforcing hard gates (profile=$PROFILE)"

COMMON_GATES=(
  "bash scripts/check_selfhost_boundary.sh --strict"
  "bash scripts/check_host_abi_conformance.sh"
  "bash scripts/check_runner_high_level_op_guard.sh"
  "bash scripts/check_prelude_capability_coverage.sh"
  "bash scripts/check_selfhost_artifact_fresh.sh"
  "bash scripts/check_no_user_panics.sh"
  "bash scripts/check_rust_engine_compat.sh"
  "bash scripts/check_source_size_budget.sh"
)

PROFILE_GATES=()
case "$PROFILE" in
  dev-fast)
    PROFILE_GATES+=("cargo test -p gc_cli --test cli_smoke --quiet")
    PROFILE_GATES+=(
      "bash scripts/test_changed_fast.sh --base HEAD --runner cargo --budget-ms ${DEV_FAST_BUDGET_MS} --min-history 1 --report .genesis/perf/upgrade_plan_dev_fast_metrics.json --history .genesis/perf/upgrade_plan_dev_fast_history.jsonl"
    )
    ;;
  prepush-standard)
    PROFILE_GATES+=("cargo clippy --workspace --all-targets -- -D warnings")
    PROFILE_GATES+=("cargo test -p gc_cli --test cli_smoke --quiet")
    PROFILE_GATES+=("cargo test -p gc_cli --test cli_gcpm_selfhost_acceptance --quiet")
    PROFILE_GATES+=("bash scripts/check_perf_budgets.sh")
    PROFILE_GATES+=("bash scripts/check_ai_iteration_slo.sh")
    PROFILE_GATES+=("bash scripts/check_runtime_microbench_budgets.sh")
    ;;
  release-full)
    PROFILE_GATES+=("cargo clippy --workspace --all-targets -- -D warnings")
    PROFILE_GATES+=("cargo test -p gc_cli --test cli_smoke --quiet")
    PROFILE_GATES+=("cargo test -p gc_cli --test cli_gcpm_selfhost_acceptance --quiet")
    PROFILE_GATES+=("bash scripts/check_perf_budgets.sh")
    PROFILE_GATES+=("bash scripts/check_ai_iteration_slo.sh")
    PROFILE_GATES+=("bash scripts/check_ai_stress_suite.sh")
    PROFILE_GATES+=("bash scripts/check_hot_path_budgets.sh")
    PROFILE_GATES+=("bash scripts/check_runtime_microbench_budgets.sh")
    ;;
esac

for cmd in "${COMMON_GATES[@]}"; do
  echo "upgrade-plan-health: >> $cmd"
  bash -lc "$cmd"
done

for cmd in "${PROFILE_GATES[@]}"; do
  echo "upgrade-plan-health: >> $cmd"
  bash -lc "$cmd"
done

echo "upgrade-plan-health: ok"
