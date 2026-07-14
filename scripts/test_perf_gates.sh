#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "test-perf-gates" \
  root-host

RUNNER="${GENESIS_TEST_PERF_GATES_RUNNER:-cargo}"
CARGO_PROFILE="${GENESIS_TEST_PERF_GATES_CARGO_PROFILE:-}"
LIST_ONLY=0
declare -a TESTS=()

DEFAULT_TESTS=(
  upgrade_plan_health
  agent_authoring_bundle_guard
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
  cmd=(cargo test -p gc_cli --test "$test_name")
  if [[ -n "$CARGO_PROFILE" ]]; then
    cmd+=(--profile "$CARGO_PROFILE")
  fi
  cmd+=(-- --ignored --test-threads=1)
  "${cmd[@]}"
done

echo "test-perf-gates: ok (${#TESTS[@]} test target(s))"
