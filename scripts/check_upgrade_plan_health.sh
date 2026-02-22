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
DEV_FAST_BUDGET_MS="${GENESIS_DEV_FAST_BUDGET_MS:-60000}"
DEV_FAST_PROFILE_WALL_BUDGET_MS="${GENESIS_HEALTH_DEV_FAST_WALL_BUDGET_MS:-300000}"
TEST_GATE_OVERRIDE="${GENESIS_HEALTH_TEST_GATE_OVERRIDE:-}"
HEALTH_PROFILE_REPORT="${GENESIS_HEALTH_PROFILE_REPORT:-.genesis/perf/upgrade_plan_health_profile_report.json}"
PREPUSH_WALL_BUDGET_MS="${GENESIS_HEALTH_PREPUSH_BUDGET_MS:-240000}"
HEALTH_CARGO_TARGET_DIR="${GENESIS_HEALTH_CARGO_TARGET_DIR:-$ROOT_DIR/.genesis/build/health/$PROFILE}"
HEALTH_CARGO_GATE_SHARDS="${GENESIS_HEALTH_CARGO_GATE_SHARDS:-1}"
HEALTH_WARM_CARGO_CACHE="${GENESIS_HEALTH_WARM_CARGO_CACHE:-auto}"
HEALTH_WARMUP_REPORT="${GENESIS_HEALTH_WARMUP_REPORT:-.genesis/perf/upgrade_plan_health_warmup_${PROFILE}.json}"
if [[ "${CI:-}" == "true" ]]; then
  ENFORCE_GATES_DEFAULT="1"
else
  ENFORCE_GATES_DEFAULT="0"
fi
ENFORCE_GATES="${GENESIS_HEALTH_ENFORCE_GATES:-$ENFORCE_GATES_DEFAULT}"
GPU_DEVICE_CONFORMANCE=""
NON_CARGO_PARTITION=()
CARGO_PARTITION=()

now_ms() {
  python3 - <<'PY'
import time
print(int(time.time() * 1000))
PY
}

detect_parallelism() {
  python3 - <<'PY'
import os
count = os.cpu_count() or 1
if count < 1:
    count = 1
print(count)
PY
}

default_health_shards_for_profile() {
  local profile="$1"
  local cpu_count
  cpu_count="$(detect_parallelism)"
  case "$profile" in
    dev-fast)
      if (( cpu_count >= 8 )); then
        echo "3"
      elif (( cpu_count >= 4 )); then
        echo "2"
      else
        echo "1"
      fi
      ;;
    prepush-standard)
      if (( cpu_count >= 4 )); then
        echo "4"
      elif (( cpu_count >= 2 )); then
        echo "2"
      else
        echo "1"
      fi
      ;;
    release-full)
      if (( cpu_count >= 8 )); then
        echo "4"
      elif (( cpu_count >= 4 )); then
        echo "3"
      elif (( cpu_count >= 2 )); then
        echo "2"
      else
        echo "1"
      fi
      ;;
    *)
      echo "1"
      ;;
  esac
}

write_health_profile_report() {
  local profile="$1"
  local configured_shards="$2"
  local gate_count="$3"
  local elapsed_ms="$4"
  local budget_ms="$5"
  local ok="$6"
  local report_path="$7"

  python3 - "$profile" "$configured_shards" "$gate_count" "$elapsed_ms" "$budget_ms" "$ok" "$report_path" <<'PY'
import json
import pathlib
import sys

profile = sys.argv[1]
configured_shards = int(sys.argv[2])
gate_count = int(sys.argv[3])
elapsed_ms = int(sys.argv[4])
budget_ms_raw = sys.argv[5].strip()
ok = sys.argv[6].strip() == "1"
report_path = pathlib.Path(sys.argv[7])
budget_ms = int(budget_ms_raw) if budget_ms_raw else None

doc = {
    "kind": "genesis/upgrade-plan-health-profile-v0.1",
    "profile": profile,
    "configured_shards": configured_shards,
    "gate_count": gate_count,
    "elapsed_ms": elapsed_ms,
    "budget_ms": budget_ms,
    "ok": ok,
}
if report_path.is_file():
    try:
        prev = json.loads(report_path.read_text(encoding="utf-8"))
        if (
            isinstance(prev, dict)
            and prev.get("kind") == "genesis/upgrade-plan-health-profile-v0.1"
            and prev.get("profile") == profile
        ):
            prev_elapsed = prev.get("elapsed_ms")
            if isinstance(prev_elapsed, int):
                doc["previous_elapsed_ms"] = prev_elapsed
                doc["elapsed_delta_ms"] = elapsed_ms - prev_elapsed
    except Exception:
        pass
report_path.parent.mkdir(parents=True, exist_ok=True)
report_path.write_text(json.dumps(doc, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"upgrade-plan-health: wrote profile report {report_path}")
PY
}

write_health_warmup_report() {
  local profile="$1"
  local mode="$2"
  local elapsed_ms="$3"
  local command_count="$4"
  local enabled="$5"
  local report_path="$6"

  python3 - "$profile" "$mode" "$elapsed_ms" "$command_count" "$enabled" "$report_path" <<'PY'
import json
import pathlib
import sys

profile = sys.argv[1]
mode = sys.argv[2]
elapsed_ms = int(sys.argv[3])
command_count = int(sys.argv[4])
enabled = sys.argv[5] == "1"
report_path = pathlib.Path(sys.argv[6])

doc = {
    "kind": "genesis/upgrade-plan-health-cargo-warmup-v0.1",
    "profile": profile,
    "mode": mode,
    "elapsed_ms": elapsed_ms,
    "command_count": command_count,
    "enabled": enabled,
}
if report_path.is_file():
    try:
        prev = json.loads(report_path.read_text(encoding="utf-8"))
        if (
            isinstance(prev, dict)
            and prev.get("kind") == "genesis/upgrade-plan-health-cargo-warmup-v0.1"
            and prev.get("profile") == profile
            and prev.get("mode") == mode
        ):
            prev_elapsed = prev.get("elapsed_ms")
            if isinstance(prev_elapsed, int):
                doc["previous_elapsed_ms"] = prev_elapsed
                doc["elapsed_delta_ms"] = elapsed_ms - prev_elapsed
    except Exception:
        pass
report_path.parent.mkdir(parents=True, exist_ok=True)
report_path.write_text(json.dumps(doc, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"upgrade-plan-health: wrote cargo warmup report {report_path}")
PY
}

command_uses_cargo() {
  local cmd="$1"
  local script_path=""

  if [[ "$cmd" == cargo* || "$cmd" == *" cargo "* ]]; then
    return 0
  fi

  if [[ "$cmd" == *" bash "* || "$cmd" == bash\ * ]]; then
    script_path="${cmd#*bash }"
    script_path="${script_path%% *}"
    if [[ -f "$script_path" ]]; then
      if command -v rg >/dev/null 2>&1; then
        if rg -q '(^|[[:space:]])cargo[[:space:]]' "$script_path"; then
          return 0
        fi
      else
        if grep -Eq '(^|[[:space:]])cargo[[:space:]]' "$script_path"; then
          return 0
        fi
      fi
    fi
  fi

  return 1
}

partition_gate_commands() {
  NON_CARGO_PARTITION=()
  CARGO_PARTITION=()

  local cmd
  for cmd in "$@"; do
    if command_uses_cargo "$cmd"; then
      CARGO_PARTITION+=("$cmd")
    else
      NON_CARGO_PARTITION+=("$cmd")
    fi
  done
}

build_cargo_warmup_commands() {
  local mode="$1"
  CARGO_WARMUP_COMMANDS=()
  CARGO_WARMUP_COMMANDS+=("cargo metadata --format-version 1 --no-deps >/dev/null")

  case "$mode" in
    mandatory-local)
      CARGO_WARMUP_COMMANDS+=("cargo test -p gc_effects --test task_concurrency_stress --no-run --quiet")
      CARGO_WARMUP_COMMANDS+=("cargo test -p gc_effects --test host_bridge_fault_injection --no-run --quiet")
      CARGO_WARMUP_COMMANDS+=("cargo check --workspace --all-targets --quiet")
      ;;
    profile:dev-fast)
      CARGO_WARMUP_COMMANDS+=("cargo test -p gc_cli --test cli_smoke --no-run --quiet")
      ;;
    profile:prepush-standard)
      CARGO_WARMUP_COMMANDS+=("cargo test -p gc_cli --test cli_smoke --no-run --quiet")
      CARGO_WARMUP_COMMANDS+=("cargo test -p gc_cli --test cli_gcpm_selfhost_acceptance --no-run --quiet")
      CARGO_WARMUP_COMMANDS+=("cargo test -p gc_cli --test cli_pkg_workspace --no-run --quiet")
      CARGO_WARMUP_COMMANDS+=("cargo check --workspace --all-targets --quiet")
      ;;
    profile:release-full)
      CARGO_WARMUP_COMMANDS+=("cargo test -p gc_cli --test cli_smoke --no-run --quiet")
      CARGO_WARMUP_COMMANDS+=("cargo test -p gc_cli --test cli_gcpm_selfhost_acceptance --no-run --quiet")
      CARGO_WARMUP_COMMANDS+=("cargo test -p gc_cli --test cli_pkg_workspace --no-run --quiet")
      CARGO_WARMUP_COMMANDS+=("cargo test -p gc_effects --test gfx_gpu_bridge --no-run --quiet")
      CARGO_WARMUP_COMMANDS+=("cargo test -p gc_effects --test host_abi_surface --no-run --quiet")
      CARGO_WARMUP_COMMANDS+=("cargo test -p gc_cli --test cli_selfhost_gpu_parallel --no-run --quiet")
      CARGO_WARMUP_COMMANDS+=("cargo check --workspace --all-targets --quiet")
      ;;
  esac
}

run_health_cargo_warmup() {
  local mode="$1"

  if [[ "$HEALTH_WARM_CARGO_CACHE" != "0" && "$HEALTH_WARM_CARGO_CACHE" != "1" ]]; then
    echo "upgrade-plan-health: GENESIS_HEALTH_WARM_CARGO_CACHE must be auto, 0, or 1" >&2
    exit 2
  fi

  build_cargo_warmup_commands "$mode"

  if [[ "$HEALTH_WARM_CARGO_CACHE" == "0" ]]; then
    write_health_warmup_report "$PROFILE" "$mode" "0" "${#CARGO_WARMUP_COMMANDS[@]}" "0" "$HEALTH_WARMUP_REPORT"
    echo "upgrade-plan-health: cargo warmup disabled (mode=${mode})"
    return 0
  fi

  if [[ "${#CARGO_WARMUP_COMMANDS[@]}" -eq 0 ]]; then
    write_health_warmup_report "$PROFILE" "$mode" "0" "0" "1" "$HEALTH_WARMUP_REPORT"
    echo "upgrade-plan-health: cargo warmup skipped (mode=${mode}, no commands)"
    return 0
  fi

  local start_ms
  local end_ms
  local elapsed_ms
  start_ms="$(now_ms)"
  local cmd
  for cmd in "${CARGO_WARMUP_COMMANDS[@]}"; do
    echo "upgrade-plan-health: [cargo-warmup ${mode}] >> $cmd"
    bash -lc "$cmd"
  done
  end_ms="$(now_ms)"
  elapsed_ms=$((end_ms - start_ms))
  write_health_warmup_report "$PROFILE" "$mode" "$elapsed_ms" "${#CARGO_WARMUP_COMMANDS[@]}" "1" "$HEALTH_WARMUP_REPORT"
  echo "upgrade-plan-health: cargo warmup elapsed_ms=${elapsed_ms} commands=${#CARGO_WARMUP_COMMANDS[@]} mode=${mode}"
}

run_gate_commands() {
  local group_label="$1"
  local shard_count="$2"
  shift 2
  local -a gate_cmds_ref=("$@")

  if (( ${#gate_cmds_ref[@]} == 0 )); then
    return 0
  fi

  if (( shard_count <= 1 || ${#gate_cmds_ref[@]} <= 1 )); then
    for cmd in "${gate_cmds_ref[@]}"; do
      echo "upgrade-plan-health: [${group_label}] >> $cmd"
      bash -lc "$cmd"
    done
    return 0
  fi

  local tmp_dir
  tmp_dir="$(mktemp -d)"
  local -a shard_files=()
  local -a pids=()
  local launched=0
  local idx
  for ((idx = 0; idx < shard_count; idx += 1)); do
    shard_files[$idx]="$tmp_dir/shard_${idx}.txt"
    : > "${shard_files[$idx]}"
  done

  for ((idx = 0; idx < ${#gate_cmds_ref[@]}; idx += 1)); do
    local shard=$((idx % shard_count))
    printf '%s\n' "${gate_cmds_ref[$idx]}" >> "${shard_files[$shard]}"
  done

  for ((idx = 0; idx < shard_count; idx += 1)); do
    local file="${shard_files[$idx]}"
    if [[ ! -s "$file" ]]; then
      continue
    fi
    launched=$((launched + 1))
    (
      while IFS= read -r cmd || [[ -n "$cmd" ]]; do
        [[ -z "$cmd" ]] && continue
        echo "upgrade-plan-health: [${group_label} shard $((idx + 1))/${shard_count}] >> $cmd"
        bash -lc "$cmd"
      done < "$file"
    ) &
    pids+=("$!")
  done

  local failed=0
  for pid in "${pids[@]}"; do
    if ! wait "$pid"; then
      failed=1
    fi
  done
  rm -rf "$tmp_dir"
  if (( failed != 0 )); then
    return 1
  fi
  echo "upgrade-plan-health: completed ${group_label} gates with deterministic sharding (${launched}/${shard_count} shards active)"
  return 0
}

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
if [[ -z "${GENESIS_HEALTH_CARGO_TARGET_DIR:-}" ]]; then
  HEALTH_CARGO_TARGET_DIR="$ROOT_DIR/.genesis/build/health/$PROFILE"
fi
if [[ -z "${GENESIS_HEALTH_WARMUP_REPORT:-}" ]]; then
  HEALTH_WARMUP_REPORT=".genesis/perf/upgrade_plan_health_warmup_${PROFILE}.json"
fi
if [[ -z "${GENESIS_HEALTH_REQUIRE_GPU_DEVICE_CONFORMANCE+x}" ]]; then
  if [[ "$PROFILE" == "release-full" ]]; then
    GPU_DEVICE_CONFORMANCE="1"
  else
    GPU_DEVICE_CONFORMANCE="0"
  fi
else
  GPU_DEVICE_CONFORMANCE="${GENESIS_HEALTH_REQUIRE_GPU_DEVICE_CONFORMANCE}"
fi
if [[ "$ENFORCE_GATES" != "0" && "$ENFORCE_GATES" != "1" ]]; then
  echo "upgrade-plan-health: GENESIS_HEALTH_ENFORCE_GATES must be 0 or 1" >&2
  exit 2
fi
if [[ "$GPU_DEVICE_CONFORMANCE" != "0" && "$GPU_DEVICE_CONFORMANCE" != "1" ]]; then
  echo "upgrade-plan-health: GENESIS_HEALTH_REQUIRE_GPU_DEVICE_CONFORMANCE must be 0 or 1" >&2
  exit 2
fi
if [[ ! "$PREPUSH_WALL_BUDGET_MS" =~ ^[0-9]+$ || "$PREPUSH_WALL_BUDGET_MS" -le 0 ]]; then
  echo "upgrade-plan-health: GENESIS_HEALTH_PREPUSH_BUDGET_MS must be a positive integer (ms)" >&2
  exit 2
fi
if [[ ! "$DEV_FAST_PROFILE_WALL_BUDGET_MS" =~ ^[0-9]+$ || "$DEV_FAST_PROFILE_WALL_BUDGET_MS" -le 0 ]]; then
  echo "upgrade-plan-health: GENESIS_HEALTH_DEV_FAST_WALL_BUDGET_MS must be a positive integer (ms)" >&2
  exit 2
fi

DEFAULT_HEALTH_SHARDS="$(default_health_shards_for_profile "$PROFILE")"
HEALTH_SHARDS="${GENESIS_HEALTH_SHARDS:-$DEFAULT_HEALTH_SHARDS}"
if [[ ! "$HEALTH_SHARDS" =~ ^[0-9]+$ || "$HEALTH_SHARDS" -le 0 ]]; then
  echo "upgrade-plan-health: GENESIS_HEALTH_SHARDS must be a positive integer" >&2
  exit 2
fi
if [[ ! "$HEALTH_CARGO_GATE_SHARDS" =~ ^[0-9]+$ || "$HEALTH_CARGO_GATE_SHARDS" -le 0 ]]; then
  echo "upgrade-plan-health: GENESIS_HEALTH_CARGO_GATE_SHARDS must be a positive integer" >&2
  exit 2
fi
if [[ "$HEALTH_WARM_CARGO_CACHE" == "auto" ]]; then
  if [[ "$PROFILE" == "dev-fast" ]]; then
    HEALTH_WARM_CARGO_CACHE="0"
  else
    HEALTH_WARM_CARGO_CACHE="1"
  fi
fi

PROFILE_SHARDS="${GENESIS_HEALTH_PROFILE_SHARDS:-$HEALTH_SHARDS}"
if [[ -z "${GENESIS_HEALTH_PROFILE_SHARDS:-}" && ( "$PROFILE" == "prepush-standard" || "$PROFILE" == "release-full" ) ]]; then
  # Profile gates include multiple cargo-heavy commands. Serial execution avoids lock contention
  # and redundant recompiles while preserving full semantic coverage.
  PROFILE_SHARDS="1"
fi
if [[ ! "$PROFILE_SHARDS" =~ ^[0-9]+$ || "$PROFILE_SHARDS" -le 0 ]]; then
  echo "upgrade-plan-health: GENESIS_HEALTH_PROFILE_SHARDS must be a positive integer" >&2
  exit 2
fi

if [[ ! -f "$PLAN_FILE" ]]; then
  echo "upgrade-plan-health: missing file: $PLAN_FILE"
  exit 1
fi

mkdir -p "$HEALTH_CARGO_TARGET_DIR"
export CARGO_TARGET_DIR="$HEALTH_CARGO_TARGET_DIR"
echo "upgrade-plan-health: using shared cargo target dir: $CARGO_TARGET_DIR"

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
  echo "upgrade-plan-health: backlog status: open checklist items = $declared_open"
  if [[ "$ENFORCE_GATES" != "1" ]]; then
    echo "upgrade-plan-health: backlog open; running mandatory local guard gates."
    run_health_cargo_warmup "mandatory-local"
    MANDATORY_LOCAL_NON_CARGO_GATES=(
      "bash scripts/check_selfhost_boundary.sh --strict"
      "bash scripts/check_selfhost_doc_runtime_parity.sh"
      "bash scripts/check_redteam_report.sh"
      "bash scripts/check_selfhost_symbol_ownership.sh"
      "bash scripts/check_selfhost_toolchain_review_fresh.sh"
      "bash scripts/check_assurance_standards_crosswalk.sh"
      "bash scripts/check_planning_docs_fresh.sh"
      "bash scripts/check_doc_hygiene.sh"
      "bash scripts/check_feature_matrix_evidence.sh"
      "bash scripts/check_write_genesiscode_skill_pack.sh"
      "bash scripts/check_no_production_rust_frontend_refs.sh"
    )
    MANDATORY_LOCAL_CARGO_GATES=(
      "bash scripts/check_task_concurrency_stress.sh"
      "bash scripts/check_host_bridge_fault_injection.sh"
      "bash scripts/check_no_user_panics.sh"
    )
    MANDATORY_LOCAL_GATES=(
      "${MANDATORY_LOCAL_NON_CARGO_GATES[@]}"
      "${MANDATORY_LOCAL_CARGO_GATES[@]}"
    )
    start_ms="$(now_ms)"
    if [[ "${#MANDATORY_LOCAL_NON_CARGO_GATES[@]}" -gt 0 ]]; then
      run_gate_commands "mandatory-local-non-cargo" "$HEALTH_SHARDS" "${MANDATORY_LOCAL_NON_CARGO_GATES[@]}"
    fi
    if [[ "${#MANDATORY_LOCAL_CARGO_GATES[@]}" -gt 0 ]]; then
      # Keep cargo-heavy checks serialized against a shared cache target dir to
      # avoid lock contention while preserving deterministic artifact reuse.
      run_gate_commands "mandatory-local-cargo" "1" "${MANDATORY_LOCAL_CARGO_GATES[@]}"
    fi
    end_ms="$(now_ms)"
    elapsed_ms=$((end_ms - start_ms))
    gate_count="${#MANDATORY_LOCAL_GATES[@]}"
    mandatory_budget=""
    mandatory_ok=1
    if [[ "$PROFILE" == "dev-fast" ]]; then
      mandatory_budget="$DEV_FAST_PROFILE_WALL_BUDGET_MS"
      if (( elapsed_ms > DEV_FAST_PROFILE_WALL_BUDGET_MS )); then
        mandatory_ok=0
      fi
    fi
    write_health_profile_report \
      "$PROFILE" \
      "$HEALTH_SHARDS" \
      "$gate_count" \
      "$elapsed_ms" \
      "$mandatory_budget" \
      "$mandatory_ok" \
      "$HEALTH_PROFILE_REPORT"
    echo "upgrade-plan-health: mandatory-local elapsed_ms=${elapsed_ms} gate_count=${gate_count}"
    if (( mandatory_ok == 0 )); then
      echo "upgrade-plan-health: dev-fast mandatory-local wall-time exceeded budget (${elapsed_ms}ms > ${DEV_FAST_PROFILE_WALL_BUDGET_MS}ms)" >&2
      exit 1
    fi
    echo "upgrade-plan-health: ok"
    exit 0
  fi
  echo "upgrade-plan-health: code health gates enforced despite backlog (profile=$PROFILE)"
else
  echo "upgrade-plan-health: backlog status: open checklist items = 0"
  echo "upgrade-plan-health: code health gates enforced (profile=$PROFILE)"
fi

COMMON_GATES=(
  "bash scripts/check_selfhost_boundary.sh --strict"
  "bash scripts/check_host_abi_conformance.sh"
  "bash scripts/check_runner_high_level_op_guard.sh"
  "bash scripts/check_prelude_capability_coverage.sh"
  "bash scripts/check_foundation_stdlib_conformance.sh"
  "bash scripts/check_capability_indices.sh"
  "bash scripts/check_assurance_profile_packs.sh"
  "bash scripts/check_assurance_standards_crosswalk.sh"
  "bash scripts/check_cargo_target_dir_policy.sh"
  "bash scripts/check_selfhost_symbol_ownership.sh"
  "bash scripts/check_agent_authoring_bundle.sh"
  "bash scripts/check_genesiscode_authoring_skill.sh"
  "bash scripts/check_domain_kit_workflows.sh"
  "bash scripts/check_selfhost_refactor_guard.sh"
  "bash scripts/check_selfhost_artifact_fresh.sh"
  "bash scripts/check_selfhost_toolchain_review_fresh.sh"
  "bash scripts/check_selfhost_dashboard_fresh.sh"
  "bash scripts/check_selfhost_doc_runtime_parity.sh"
  "bash scripts/check_redteam_report.sh"
  "bash scripts/check_feature_matrix_gap_hygiene.sh"
  "bash scripts/check_feature_matrix_evidence.sh"
  "bash scripts/check_planning_docs_fresh.sh"
  "bash scripts/check_doc_hygiene.sh"
  "bash scripts/check_write_genesiscode_skill_pack.sh"
  "bash scripts/check_task_concurrency_stress.sh"
  "bash scripts/check_host_bridge_fault_injection.sh"
  "bash scripts/check_no_user_panics.sh"
  "bash scripts/check_rust_engine_compat.sh"
  "bash scripts/check_no_production_rust_frontend_refs.sh"
  "bash scripts/check_production_cli_help_surface.sh"
  "bash scripts/check_cli_diagnostics_contract.sh"
  "bash scripts/check_fuzz_differential_hardening.sh"
  "bash scripts/check_test_execution_profile_matrix.sh"
  "bash scripts/check_gpu_conformance_lane_matrix.sh"
  "bash scripts/check_webxr_browser_conformance_lane.sh"
  "bash scripts/check_gc_source_size_budget.sh"
  "bash scripts/check_source_size_budget.sh"
  "bash scripts/check_test_size_budget.sh"
)

PROFILE_GATES=()
case "$PROFILE" in
  dev-fast)
    PROFILE_GATES+=("cargo test -p gc_cli --test cli_smoke --quiet")
    PROFILE_GATES+=(
      "bash scripts/test_changed_fast.sh --base HEAD --runner auto --budget-ms ${DEV_FAST_BUDGET_MS} --min-history 1 --report .genesis/perf/upgrade_plan_dev_fast_metrics.json --history .genesis/perf/upgrade_plan_dev_fast_history.jsonl"
    )
    ;;
  prepush-standard)
    PROFILE_GATES+=("cargo clippy --workspace --all-targets -- -D warnings")
    PROFILE_GATES+=("cargo test -p gc_cli --test cli_smoke --quiet")
    PROFILE_GATES+=("cargo test -p gc_cli --test cli_gcpm_selfhost_acceptance --quiet")
    PROFILE_GATES+=("cargo test -p gc_cli --test cli_pkg_workspace gcpm_build_target_is_reproducible_and_emits_provenance_bundle --quiet")
    PROFILE_GATES+=("bash scripts/check_runtime_backend_feature_matrix.sh")
    PROFILE_GATES+=("GENESIS_AGENT_GAUNTLET_PROFILE=prepush-standard bash scripts/check_agent_reference_workflows.sh")
    PROFILE_GATES+=("bash scripts/check_agent_generative_workloads.sh")
    PROFILE_GATES+=("bash scripts/check_perf_budgets.sh")
    PROFILE_GATES+=("bash scripts/check_ai_iteration_slo.sh")
    PROFILE_GATES+=("bash scripts/check_runtime_microbench_budgets.sh")
    PROFILE_GATES+=("bash scripts/check_gpu_compute_runtime_profile.sh")
    ;;
  release-full)
    PROFILE_GATES+=("cargo clippy --workspace --all-targets -- -D warnings")
    PROFILE_GATES+=("cargo test -p gc_cli --test cli_smoke --quiet")
    PROFILE_GATES+=("cargo test -p gc_cli --test cli_gcpm_selfhost_acceptance --quiet")
    PROFILE_GATES+=("cargo test -p gc_cli --test cli_pkg_workspace gcpm_build_target_is_reproducible_and_emits_provenance_bundle --quiet")
    PROFILE_GATES+=("bash scripts/check_runtime_backend_feature_matrix.sh")
    PROFILE_GATES+=(
      "GENESIS_AGENT_GAUNTLET_PROFILE=release-full GENESIS_AGENT_GAUNTLET_REQUIRE_GPU_DEVICE_BACKEND=1 bash scripts/check_agent_reference_workflows.sh"
    )
    PROFILE_GATES+=("bash scripts/check_agent_scenario_perf.sh")
    PROFILE_GATES+=("bash scripts/check_agent_generative_workloads.sh")
    PROFILE_GATES+=("GENESIS_AGENT_PARITY_GAUNTLET_PROFILE=prepush-standard bash scripts/check_agent_workflow_runtime_parity.sh")
    PROFILE_GATES+=("bash scripts/check_perf_budgets.sh")
    PROFILE_GATES+=("bash scripts/check_ai_iteration_slo.sh")
    PROFILE_GATES+=("bash scripts/check_ai_stress_suite.sh")
    PROFILE_GATES+=("bash scripts/check_hot_path_budgets.sh")
    PROFILE_GATES+=("bash scripts/check_runtime_microbench_budgets.sh")
    PROFILE_GATES+=("bash scripts/check_gpu_compute_runtime_profile.sh")
    ;;
esac

if [[ "$GPU_DEVICE_CONFORMANCE" == "1" ]]; then
  PROFILE_GATES+=("bash scripts/check_gpu_compute_device_conformance.sh")
  PROFILE_GATES+=(
    "GENESIS_GPU_DEVICE_CONFORMANCE_OUT_DIR=.genesis/perf/gpu_device_conformance_deterministic GENESIS_GPU_DEVICE_CONFORMANCE_REPORT_OUT=.genesis/perf/gpu_device_conformance_deterministic_report.json GENESIS_GPU_DEVICE_CONFORMANCE_FEATURES= GENESIS_GPU_COMPUTE_BACKEND_POLICY=require-device GENESIS_GPU_COMPUTE_DEVICE_RUNTIME_CMD=$ROOT_DIR/scripts/gpu_device_runtime_deterministic.sh GENESIS_RUNTIME_MICROBENCH_REQUIRED_GPU_BACKEND=device-runtime bash scripts/check_gpu_compute_device_conformance.sh"
  )
  PROFILE_GATES+=(
    "bash scripts/check_gpu_device_conformance_lane_parity.sh --lane-a .genesis/perf/gpu_device_conformance_report.json --lane-b .genesis/perf/gpu_device_conformance_deterministic_report.json --out .genesis/perf/gpu_device_lane_parity_report.json"
  )
fi

if [[ -n "$TEST_GATE_OVERRIDE" ]]; then
  COMMON_GATES=("$TEST_GATE_OVERRIDE")
  PROFILE_GATES=()
fi

run_health_cargo_warmup "profile:${PROFILE}"

partition_gate_commands "${COMMON_GATES[@]}"
start_ms="$(now_ms)"
common_non_cargo_gate_count="${#NON_CARGO_PARTITION[@]}"
common_cargo_gate_count="${#CARGO_PARTITION[@]}"

if (( common_non_cargo_gate_count > 0 )); then
  echo "upgrade-plan-health: running ${common_non_cargo_gate_count} common non-cargo gates (profile=${PROFILE}, shards=${HEALTH_SHARDS})"
  run_gate_commands "common-non-cargo" "$HEALTH_SHARDS" "${NON_CARGO_PARTITION[@]}"
fi
if (( common_cargo_gate_count > 0 )); then
  echo "upgrade-plan-health: running ${common_cargo_gate_count} common cargo gates (profile=${PROFILE}, shards=${HEALTH_CARGO_GATE_SHARDS})"
  run_gate_commands "common-cargo" "$HEALTH_CARGO_GATE_SHARDS" "${CARGO_PARTITION[@]}"
fi

if (( ${#PROFILE_GATES[@]} > 0 )); then
  partition_gate_commands "${PROFILE_GATES[@]}"
else
  # Bash 3 + nounset treats "${arr[@]}" on empty arrays as unbound. Call with zero args explicitly.
  partition_gate_commands
fi
profile_non_cargo_gate_count="${#NON_CARGO_PARTITION[@]}"
profile_cargo_gate_count="${#CARGO_PARTITION[@]}"

if (( profile_non_cargo_gate_count > 0 )); then
  echo "upgrade-plan-health: running ${profile_non_cargo_gate_count} profile non-cargo gates (profile=${PROFILE}, shards=${PROFILE_SHARDS})"
  run_gate_commands "profile:${PROFILE}:non-cargo" "$PROFILE_SHARDS" "${NON_CARGO_PARTITION[@]}"
fi
if (( profile_cargo_gate_count > 0 )); then
  echo "upgrade-plan-health: running ${profile_cargo_gate_count} profile cargo gates (profile=${PROFILE}, shards=${HEALTH_CARGO_GATE_SHARDS})"
  run_gate_commands "profile:${PROFILE}:cargo" "$HEALTH_CARGO_GATE_SHARDS" "${CARGO_PARTITION[@]}"
fi
end_ms="$(now_ms)"
elapsed_ms=$((end_ms - start_ms))
gate_count=$(( \
  common_non_cargo_gate_count + \
  common_cargo_gate_count + \
  profile_non_cargo_gate_count + \
  profile_cargo_gate_count \
))

profile_budget=""
profile_ok=1
if [[ "$PROFILE" == "dev-fast" ]]; then
  profile_budget="$DEV_FAST_PROFILE_WALL_BUDGET_MS"
  if (( elapsed_ms > DEV_FAST_PROFILE_WALL_BUDGET_MS )); then
    profile_ok=0
  fi
elif [[ "$PROFILE" == "prepush-standard" ]]; then
  profile_budget="$PREPUSH_WALL_BUDGET_MS"
  if (( elapsed_ms > PREPUSH_WALL_BUDGET_MS )); then
    profile_ok=0
  fi
fi

write_health_profile_report \
  "$PROFILE" \
  "$HEALTH_SHARDS" \
  "$gate_count" \
  "$elapsed_ms" \
  "$profile_budget" \
  "$profile_ok" \
  "$HEALTH_PROFILE_REPORT"

if (( profile_ok == 0 )); then
  if [[ "$PROFILE" == "dev-fast" ]]; then
    echo "upgrade-plan-health: dev-fast wall-time exceeded budget (${elapsed_ms}ms > ${DEV_FAST_PROFILE_WALL_BUDGET_MS}ms)" >&2
  elif [[ "$PROFILE" == "prepush-standard" ]]; then
    echo "upgrade-plan-health: prepush wall-time exceeded budget (${elapsed_ms}ms > ${PREPUSH_WALL_BUDGET_MS}ms)" >&2
  else
    echo "upgrade-plan-health: profile wall-time exceeded budget (${elapsed_ms}ms > ${profile_budget}ms)" >&2
  fi
  exit 1
fi

echo "upgrade-plan-health: elapsed_ms=${elapsed_ms} gate_count=${gate_count} shards=${HEALTH_SHARDS}"
echo "upgrade-plan-health: ok"
