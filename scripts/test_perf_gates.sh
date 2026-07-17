#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
source "$ROOT_DIR/scripts/lib/profile_gate_timing.sh"
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "test-perf-gates" \
  root-host

RUNNER="${GENESIS_TEST_PERF_GATES_RUNNER:-cargo}"
CARGO_PROFILE="${GENESIS_TEST_PERF_GATES_CARGO_PROFILE:-}"
LIST_ONLY=0
KERNEL_TAIL_STRESS=0
declare -a TESTS=()

DEFAULT_TESTS=(
  upgrade_plan_health
  agent_authoring_bundle_guard
  cli_agent_benchmark_scoring
  pkg_low_semantic_boundary
  guard_extraction_fixtures
  large_workspace_agent_perf
  runtime_microbench_gpu_policy
  ai_stress_suite_fault_inject
  genesiscode_authoring_skill_guard
  ai_iteration_slo_regression
  default_iteration_workflow
  shell_gate_regressions
)

usage() {
  cat <<'EOF'
Usage: scripts/test_perf_gates.sh [options]

Runs integration tests marked `#[ignore = "perf-gate"]`. These tests execute
repo-level shell gates, SLO checks, or nested cargo-backed workflows and are
intentionally excluded from default `cargo test --workspace`.

Options:
  --test <name>        run one ignored integration-test target; repeatable
  --runner <name>      runner name, currently cargo only (default: cargo)
  --cargo-profile <p>  optional cargo profile passed to cargo test
  --kernel-tail-stress run only the bounded 10M-per-tier kernel tail proof
  --list               print default ignored test targets and exit
  -h, --help           show this help
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --test)
      TESTS+=("${2:-}")
      shift 2
      ;;
    --runner)
      RUNNER="${2:-}"
      shift 2
      ;;
    --cargo-profile)
      CARGO_PROFILE="${2:-}"
      shift 2
      ;;
    --list)
      LIST_ONLY=1
      shift
      ;;
    --kernel-tail-stress)
      KERNEL_TAIL_STRESS=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "test-perf-gates: unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if (( KERNEL_TAIL_STRESS == 1 )); then
  if [[ "${#TESTS[@]}" -ne 0 || -n "$CARGO_PROFILE" || "$LIST_ONLY" -ne 0 ]]; then
    echo "test-perf-gates: --kernel-tail-stress cannot be combined with --test, --cargo-profile, or --list" >&2
    exit 2
  fi
  if [[ "$RUNNER" != "cargo" ]]; then
    echo "test-perf-gates: --kernel-tail-stress requires the cargo runner" >&2
    exit 2
  fi

  budget_ms="${GENESIS_KERNEL_TAIL_STRESS_BUDGET_MS:-300000}"
  disk_budget_bytes="${GENESIS_KERNEL_TAIL_STRESS_DISK_BUDGET_BYTES:-536870912}"
  [[ "$budget_ms" =~ ^[0-9]+$ && "$budget_ms" -gt 0 ]] || {
    echo "test-perf-gates: GENESIS_KERNEL_TAIL_STRESS_BUDGET_MS must be a positive integer" >&2
    exit 2
  }
  [[ "$disk_budget_bytes" =~ ^[0-9]+$ && "$disk_budget_bytes" -gt 0 ]] || {
    echo "test-perf-gates: GENESIS_KERNEL_TAIL_STRESS_DISK_BUDGET_BYTES must be a positive integer" >&2
    exit 2
  }

  bash scripts/check_kernel_tcb_contract.sh
  disk_before_bytes="$(( $(du -sk "$CARGO_TARGET_DIR" 2>/dev/null | awk '{print $1}') * 1024 ))"
  start_ms="$(genesis_profile_gate_now_ms)"
  cargo test -p gc_kernel \
    --profile selfhost-strict \
    --locked \
    --offline \
    tests::tail_loop_ten_million_iterations_has_constant_evaluator_depth \
    -- \
    --ignored \
    --exact \
    --test-threads=1
  end_ms="$(genesis_profile_gate_now_ms)"
  disk_after_bytes="$(( $(du -sk "$CARGO_TARGET_DIR" | awk '{print $1}') * 1024 ))"
  elapsed_ms="$((end_ms - start_ms))"
  generated_disk_bytes="$((disk_after_bytes - disk_before_bytes))"
  (( generated_disk_bytes >= 0 )) || generated_disk_bytes=0
  (( elapsed_ms <= budget_ms )) || {
    echo "test-perf-gates: kernel tail stress exceeded wall budget (${elapsed_ms}ms > ${budget_ms}ms)" >&2
    exit 1
  }
  (( generated_disk_bytes <= disk_budget_bytes )) || {
    echo "test-perf-gates: kernel tail stress exceeded generated-disk budget (${generated_disk_bytes}B > ${disk_budget_bytes}B)" >&2
    exit 1
  }
  echo "test-perf-gates: kernel-tail-stress ok iterations_per_mode=10000000 exact_steps_per_mode=90000009 max_evaluator_depth=3 elapsed_ms=${elapsed_ms} budget_ms=${budget_ms} generated_disk_bytes=${generated_disk_bytes} disk_budget_bytes=${disk_budget_bytes}"
  exit 0
fi

if [[ "${#TESTS[@]}" -eq 0 ]]; then
  TESTS=("${DEFAULT_TESTS[@]}")
fi

if (( LIST_ONLY == 1 )); then
  printf '%s\n' "${TESTS[@]}"
  exit 0
fi

if [[ "$RUNNER" != "cargo" ]]; then
  echo "test-perf-gates: --runner currently supports cargo only" >&2
  exit 2
fi

for test_name in "${TESTS[@]}"; do
  [[ -n "$test_name" ]] || {
    echo "test-perf-gates: empty --test value" >&2
    exit 2
  }

  echo "test-perf-gates: cargo test -p gc_cli --test ${test_name} -- --ignored --test-threads=1"
  start_ms="$(genesis_profile_gate_now_ms)"
  cmd=(cargo test -p gc_cli --test "$test_name")
  if [[ -n "$CARGO_PROFILE" ]]; then
    cmd+=(--profile "$CARGO_PROFILE")
  fi
  cmd+=(-- --ignored --test-threads=1)
  "${cmd[@]}"
  end_ms="$(genesis_profile_gate_now_ms)"
  elapsed_ms="$((end_ms - start_ms))"
  if [[ "$test_name" == "cli_agent_benchmark_scoring" ]]; then
    scoring_budget_ms="${GENESIS_SCORING_MATRIX_BUDGET_MS:-600000}"
    [[ "$scoring_budget_ms" =~ ^[0-9]+$ && "$scoring_budget_ms" -gt 0 ]] || {
      echo "test-perf-gates: GENESIS_SCORING_MATRIX_BUDGET_MS must be a positive integer" >&2
      exit 2
    }
    (( elapsed_ms <= scoring_budget_ms )) || {
      echo "test-perf-gates: scoring matrix exceeded wall budget (${elapsed_ms}ms > ${scoring_budget_ms}ms)" >&2
      exit 1
    }
    echo "test-perf-gates: scoring-matrix ok elapsed_ms=${elapsed_ms} budget_ms=${scoring_budget_ms} scorer_process_timeout_ms=30000"
  fi
done

echo "test-perf-gates: ok (${#TESTS[@]} test target(s))"
