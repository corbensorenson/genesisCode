#!/usr/bin/env bash
set -euo pipefail

export LC_ALL=C

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"
source "$ROOT_DIR/scripts/lib/agent_gpu_profile_contract.sh"

PLAN_FILE="upgrade_plan.md"
DEFAULT_PROFILE="dev-fast"
if [[ "${CI:-}" == "true" ]]; then
  DEFAULT_PROFILE="release-full"
fi
PROFILE="${GENESIS_HEALTH_PROFILE:-$DEFAULT_PROFILE}"
export GENESIS_HEALTH_PROFILE="$PROFILE"
AGENT_AUTOMATION_CONTEXT="$(genesis_resolve_agent_automation_context "$PROFILE")"
export GENESIS_AGENT_AUTOMATION_CONTEXT="$AGENT_AUTOMATION_CONTEXT"
AGENT_GPU_PROFILE="${GENESIS_AGENT_GPU_PROFILE:-}"
HEALTH_GPU_BACKEND_POLICY_DEFAULT="${GENESIS_HEALTH_GPU_BACKEND_POLICY_DEFAULT:-}"
DEV_FAST_BUDGET_MS="${GENESIS_DEV_FAST_BUDGET_MS:-60000}"
DEV_FAST_PROFILE_WALL_BUDGET_MS="${GENESIS_HEALTH_DEV_FAST_WALL_BUDGET_MS:-240000}"
AGENT_INNER_LOOP_BUDGET_MS="${GENESIS_HEALTH_AGENT_INNER_LOOP_BUDGET_MS:-300000}"
AGENT_INNER_LOOP_HISTORY="${GENESIS_HEALTH_AGENT_INNER_LOOP_HISTORY:-.genesis/perf/upgrade_plan_health_agent_inner_loop_history.jsonl}"
AGENT_INNER_LOOP_BASELINE_HISTORY="${GENESIS_HEALTH_AGENT_INNER_LOOP_BASELINE_HISTORY:-policies/perf/upgrade_plan_health_agent_inner_loop_seed_history.jsonl}"
AGENT_INNER_LOOP_MIN_HISTORY="${GENESIS_HEALTH_AGENT_INNER_LOOP_MIN_HISTORY:-5}"
AGENT_INNER_LOOP_REQUIRE_MIN_HISTORY="${GENESIS_HEALTH_AGENT_INNER_LOOP_REQUIRE_MIN_HISTORY:-1}"
TEST_GATE_OVERRIDE="${GENESIS_HEALTH_TEST_GATE_OVERRIDE:-}"
HEALTH_PROFILE_REPORT="${GENESIS_HEALTH_PROFILE_REPORT:-.genesis/perf/upgrade_plan_health_profile_report.json}"
HEALTH_PROFILE_HISTORY="${GENESIS_HEALTH_PROFILE_HISTORY:-.genesis/perf/upgrade_plan_health_profile_history.jsonl}"
HEALTH_PROFILE_MIN_HISTORY="${GENESIS_HEALTH_PROFILE_MIN_HISTORY:-5}"
# prepush-standard includes end-to-end agent/runtime/perf conformance lanes with
# selfhost-strict compilation + microbench suites; keep a bounded default that
# reflects full strict scope on clean runs.
PREPUSH_WALL_BUDGET_MS="${GENESIS_HEALTH_PREPUSH_BUDGET_MS:-900000}"
PREPUSH_HISTORY="${GENESIS_HEALTH_PREPUSH_HISTORY:-.genesis/perf/upgrade_plan_health_prepush_history.jsonl}"
PREPUSH_BASELINE_HISTORY="${GENESIS_HEALTH_PREPUSH_BASELINE_HISTORY:-}"
PREPUSH_MIN_HISTORY="${GENESIS_HEALTH_PREPUSH_MIN_HISTORY:-3}"
PREPUSH_REQUIRE_MIN_HISTORY="${GENESIS_HEALTH_PREPUSH_REQUIRE_MIN_HISTORY:-0}"
PREPUSH_HISTORY_SCOPE_KEY="${GENESIS_HEALTH_PREPUSH_HISTORY_SCOPE_KEY:-prepush-standard-v1}"
RELEASE_FULL_WALL_BUDGET_MS="${GENESIS_HEALTH_RELEASE_FULL_BUDGET_MS:-1800000}"
RELEASE_FULL_HISTORY="${GENESIS_HEALTH_RELEASE_FULL_HISTORY:-.genesis/perf/upgrade_plan_health_release_full_history.jsonl}"
RELEASE_FULL_BASELINE_HISTORY="${GENESIS_HEALTH_RELEASE_FULL_BASELINE_HISTORY:-}"
RELEASE_FULL_MIN_HISTORY="${GENESIS_HEALTH_RELEASE_FULL_MIN_HISTORY:-3}"
RELEASE_FULL_REQUIRE_MIN_HISTORY="${GENESIS_HEALTH_RELEASE_FULL_REQUIRE_MIN_HISTORY:-0}"
RELEASE_FULL_HISTORY_SCOPE_KEY="${GENESIS_HEALTH_RELEASE_FULL_HISTORY_SCOPE_KEY:-release-full-v1}"
HEALTH_CARGO_TARGET_DIR="${GENESIS_HEALTH_CARGO_TARGET_DIR:-$ROOT_DIR/.genesis/build/health/$PROFILE}"
HEALTH_CARGO_GATE_SHARDS="${GENESIS_HEALTH_CARGO_GATE_SHARDS:-}"
HEALTH_WARM_CARGO_CACHE="${GENESIS_HEALTH_WARM_CARGO_CACHE:-auto}"
HEALTH_WARMUP_REPORT="${GENESIS_HEALTH_WARMUP_REPORT:-.genesis/perf/upgrade_plan_health_warmup_${PROFILE}.json}"
HEALTH_PROFILE_GATE_CACHE="${GENESIS_HEALTH_PROFILE_GATE_CACHE:-auto}"
HEALTH_PROFILE_GATE_CACHE_TTL_SEC="${GENESIS_HEALTH_PROFILE_GATE_CACHE_TTL_SEC:-21600}"
if [[ "${CI:-}" == "true" ]]; then
  ENFORCE_GATES_DEFAULT="1"
else
  ENFORCE_GATES_DEFAULT="0"
fi
ENFORCE_GATES="${GENESIS_HEALTH_ENFORCE_GATES:-$ENFORCE_GATES_DEFAULT}"
HEALTH_AUTO_RECLAIM="${GENESIS_HEALTH_AUTO_RECLAIM:-1}"
HEALTH_RECLAIM_MAX_BUILD_KB="${GENESIS_HEALTH_RECLAIM_MAX_BUILD_KB:-33554432}"
HEALTH_RECLAIM_MAX_AGE_DAYS="${GENESIS_HEALTH_RECLAIM_MAX_AGE_DAYS:-7}"
HEALTH_MIN_FREE_KB="${GENESIS_HEALTH_MIN_FREE_KB:-1048576}"
HEALTH_PARALLEL_CARGO_MIN_FREE_KB="${GENESIS_HEALTH_PARALLEL_CARGO_MIN_FREE_KB:-2097152}"
HEALTH_AUTO_AGGRESSIVE_RECLAIM_ON_LOW_DISK="${GENESIS_HEALTH_AUTO_AGGRESSIVE_RECLAIM_ON_LOW_DISK:-1}"
HEALTH_STRICT_DISK_POLICY="${GENESIS_HEALTH_STRICT_DISK_POLICY:-fail}"
HEALTH_STRICT_RUNTIME_MIN_FREE_KB="${GENESIS_HEALTH_STRICT_RUNTIME_MIN_FREE_KB:-3145728}"
HEALTH_DISK_PREFLIGHT_REPORT="${GENESIS_HEALTH_DISK_PREFLIGHT_REPORT:-.genesis/perf/upgrade_plan_health_disk_preflight_report.json}"
HEALTH_DISK_PREFLIGHT_REASON="ok"
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

free_kb_root() {
  df -Pk "$ROOT_DIR" | awk 'NR==2 {print $4}'
}

default_health_shards_for_profile() {
  local profile="$1"
  local cpu_count
  cpu_count="$(detect_parallelism)"
  case "$profile" in
    agent-inner-loop)
      if (( cpu_count >= 8 )); then
        echo "3"
      elif (( cpu_count >= 4 )); then
        echo "2"
      else
        echo "1"
      fi
      ;;
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
      if (( cpu_count >= 8 )); then
        echo "6"
      elif (( cpu_count >= 4 )); then
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
    full-selfhost-cutover)
      if (( cpu_count >= 4 )); then
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

default_health_cargo_gate_shards_for_profile() {
  local profile="$1"
  local cpu_count
  cpu_count="$(detect_parallelism)"
  case "$profile" in
    prepush-standard)
      # Prepush cargo gates share one target dir for deterministic artifact reuse.
      # Running them in parallel shards causes lock contention and large wall-time
      # inflation on hot caches; serialize by default and allow explicit override.
      echo "1"
      ;;
    release-full)
      if (( cpu_count >= 8 )); then
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

health_profile_gate_cache_spec() {
  local cmd="$1"
  case "$cmd" in
    *"bash scripts/check_agent_reference_workflows.sh"*)
      cat <<'EOF'
agent-reference-workflows
scripts/check_agent_reference_workflows.sh
scripts/check_agent_gpu_profile_contract.sh
scripts/lib/agent_gpu_profile_contract.sh
prelude/modules/**/*.gc
crates/gc_cli_driver/src/**/*.rs
crates/gc_effects/src/**/*.rs
crates/gc_kernel/src/**/*.rs
crates/gc_prelude/src/**/*.rs
EOF
      return 0
      ;;
    *"bash scripts/check_gpu_xr_productization_kits.sh"*)
      cat <<'EOF'
gpu-xr-productization-kits
scripts/check_gpu_xr_productization_kits.sh
scripts/check_agent_reference_workflows.sh
scripts/check_webxr_browser_conformance.sh
prelude/modules/**/*.gc
crates/gc_gfx/src/**/*.rs
crates/gc_effects/src/**/*.rs
crates/gc_runtime/src/**/*.rs
EOF
      return 0
      ;;
    *"bash scripts/check_agent_generative_workloads.sh"*)
      cat <<'EOF'
agent-generative-workloads
scripts/check_agent_generative_workloads.sh
scripts/check_agent_reference_workflows.sh
prelude/modules/**/*.gc
crates/gc_cli_driver/src/**/*.rs
crates/gc_effects/src/**/*.rs
crates/gc_kernel/src/**/*.rs
EOF
      return 0
      ;;
    *"bash scripts/check_ai_iteration_slo.sh"*)
      cat <<'EOF'
ai-iteration-slo
scripts/check_ai_iteration_slo.sh
scripts/test_changed_fast.sh
scripts/check_disk_headroom.sh
scripts/lib/**/*.sh
crates/gc_cli/src/**/*.rs
crates/gc_cli_driver/src/**/*.rs
crates/gc_coreform/src/**/*.rs
crates/gc_kernel/src/**/*.rs
crates/gc_types/src/**/*.rs
EOF
      return 0
      ;;
    *"bash scripts/check_large_workspace_agent_perf.sh"*)
      cat <<'EOF'
large-workspace-agent-perf
scripts/check_large_workspace_agent_perf.sh
scripts/lib/**/*.sh
crates/gc_cli/src/**/*.rs
crates/gc_cli_driver/src/**/*.rs
crates/gc_prelude/src/**/*.rs
selfhost/**/*.gc
EOF
      return 0
      ;;
    *"bash scripts/check_perf_budgets.sh"*)
      cat <<'EOF'
perf-budgets
scripts/check_perf_budgets.sh
scripts/lib/**/*.sh
examples/hello_pkg/**/*.gc
selfhost/**/*.gc
crates/gc_cli/src/**/*.rs
crates/gc_cli_driver/src/**/*.rs
crates/gc_obligations/src/**/*.rs
crates/gc_types/src/**/*.rs
EOF
      return 0
      ;;
    *"GENESIS_PRODUCTION_CLI_HELP_SURFACE_INCLUDE_PARITY=0 bash scripts/check_production_cli_help_surface.sh"*)
      cat <<'EOF'
production-cli-help-surface
scripts/check_production_cli_help_surface.sh
scripts/lib/**/*.sh
crates/gc_cli/src/**/*.rs
crates/gc_cli_driver/src/**/*.rs
crates/gc_wasi_cli/src/**/*.rs
EOF
      return 0
      ;;
    *"GENESIS_PRODUCTION_CLI_HELP_SURFACE_INCLUDE_PARITY=1 "*)
      cat <<'EOF'
production-cli-help-surface-parity
scripts/check_production_cli_help_surface.sh
scripts/lib/**/*.sh
crates/gc_cli/src/**/*.rs
crates/gc_cli_driver/src/**/*.rs
crates/gc_cli_driver_parity/src/**/*.rs
crates/gc_wasi_cli/src/**/*.rs
EOF
      return 0
      ;;
    *"bash scripts/check_runtime_microbench_budgets.sh"*)
      cat <<'EOF'
runtime-microbench-budgets
scripts/check_runtime_microbench_budgets.sh
scripts/lib/**/*.sh
crates/gc_runtime_bench/src/**/*.rs
crates/gc_runtime/src/**/*.rs
crates/gc_effects/src/**/*.rs
EOF
      return 0
      ;;
    *"bash scripts/check_gpu_compute_runtime_profile.sh"*)
      cat <<'EOF'
gpu-compute-runtime-profile
scripts/check_gpu_compute_runtime_profile.sh
scripts/lib/**/*.sh
crates/gc_runtime_bench/src/**/*.rs
crates/gc_effects/src/**/*.rs
crates/gc_runtime/src/**/*.rs
prelude/modules/**/*.gc
EOF
      return 0
      ;;
    *"bash scripts/check_gfx_runtime_profile.sh"*)
      cat <<'EOF'
gfx-runtime-profile
scripts/check_gfx_runtime_profile.sh
scripts/lib/**/*.sh
crates/gc_gfx/src/**/*.rs
crates/gc_effects/src/**/*.rs
crates/gc_runtime/src/**/*.rs
prelude/modules/**/*.gc
EOF
      return 0
      ;;
    *"bash scripts/check_gpu_gfx_headroom_conformance.sh"*)
      cat <<'EOF'
gpu-gfx-headroom-conformance
scripts/check_gpu_gfx_headroom_conformance.sh
scripts/check_gpu_compute_runtime_profile.sh
scripts/check_gfx_runtime_profile.sh
scripts/lib/**/*.sh
crates/gc_gfx/src/**/*.rs
crates/gc_effects/src/**/*.rs
crates/gc_runtime_bench/src/**/*.rs
prelude/modules/**/*.gc
EOF
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

compute_health_gate_fingerprint() {
  local cmd="$1"
  shift
  python3 - "$ROOT_DIR" "$cmd" "$@" <<'PY'
import hashlib
import pathlib
import sys

root = pathlib.Path(sys.argv[1])
cmd = sys.argv[2]
patterns = [p for p in sys.argv[3:] if p]

h = hashlib.sha256()
h.update(cmd.encode("utf-8"))
h.update(b"\n")

files: set[pathlib.Path] = set()
for pattern in patterns:
    for path in root.glob(pattern):
        if path.is_file():
            files.add(path)

for path in sorted(files, key=lambda p: p.as_posix()):
    rel = path.relative_to(root).as_posix()
    h.update(rel.encode("utf-8"))
    h.update(b"\0")
    h.update(path.read_bytes())
    h.update(b"\0")

h.update(b"patterns\0")
for pattern in patterns:
    h.update(pattern.encode("utf-8"))
    h.update(b"\0")

print(h.hexdigest())
PY
}

maybe_wrap_profile_gate_with_cache() {
  local cmd="$1"
  if [[ "$HEALTH_PROFILE_GATE_CACHE" != "1" ]]; then
    echo "$cmd"
    return 0
  fi
  if [[ "$PROFILE" != "prepush-standard" ]]; then
    echo "$cmd"
    return 0
  fi

  local spec
  if ! spec="$(health_profile_gate_cache_spec "$cmd")"; then
    echo "$cmd"
    return 0
  fi

  local key
  key="$(printf '%s\n' "$spec" | sed -n '1p')"
  if [[ -z "$key" ]]; then
    echo "$cmd"
    return 0
  fi

  local -a patterns=()
  local line
  while IFS= read -r line || [[ -n "$line" ]]; do
    [[ -z "$line" ]] && continue
    patterns+=("$line")
  done < <(printf '%s\n' "$spec" | tail -n +2)

  local fingerprint
  fingerprint="$(compute_health_gate_fingerprint "$cmd" "${patterns[@]}")"
  local cmd_b64
  cmd_b64="$(printf '%s' "$cmd" | base64 | tr -d '\n')"

  echo "bash scripts/lib/run_cached_health_gate.sh --profile $PROFILE --key $key --fingerprint $fingerprint --ttl-sec $HEALTH_PROFILE_GATE_CACHE_TTL_SEC --cmd-b64 $cmd_b64"
}

apply_profile_gate_cache_policy() {
  if [[ "$HEALTH_PROFILE_GATE_CACHE" != "1" ]]; then
    return 0
  fi
  if [[ "$PROFILE" != "prepush-standard" ]]; then
    return 0
  fi
  if ! declare -p PROFILE_GATES >/dev/null 2>&1; then
    return 0
  fi
  if (( ${#PROFILE_GATES[@]} == 0 )); then
    return 0
  fi
  local -a wrapped=()
  local cmd
  for cmd in "${PROFILE_GATES[@]}"; do
    wrapped+=("$(maybe_wrap_profile_gate_with_cache "$cmd")")
  done
  PROFILE_GATES=("${wrapped[@]}")
}

enforce_inner_loop_history_budget() {
  local elapsed_ms="$1"
  local gate_count="$2"
  local profile_gate_cache_enabled=0
  local warm_cargo_cache_enabled=0
  if [[ "$HEALTH_PROFILE_GATE_CACHE" == "1" ]]; then
    profile_gate_cache_enabled=1
  fi
  if [[ "$HEALTH_WARM_CARGO_CACHE" == "1" ]]; then
    warm_cargo_cache_enabled=1
  fi
  local -a args=(
    scripts/lib/profile_runtime_budget.py
    --profile "$PROFILE"
    --kind "genesis/upgrade-plan-health-profile-v0.1"
    --report "$HEALTH_PROFILE_REPORT"
    --history "$AGENT_INNER_LOOP_HISTORY"
    --elapsed-ms "$elapsed_ms"
    --budget-ms "$AGENT_INNER_LOOP_BUDGET_MS"
    --min-history "$AGENT_INNER_LOOP_MIN_HISTORY"
    --baseline-history "$AGENT_INNER_LOOP_BASELINE_HISTORY"
    --extra-json "{\"configured_shards\":$HEALTH_SHARDS,\"profile_shards\":$PROFILE_SHARDS,\"cargo_gate_shards\":$HEALTH_CARGO_GATE_SHARDS,\"gate_count\":$gate_count,\"profile_non_cargo_gate_count\":$profile_non_cargo_gate_count,\"profile_cargo_gate_count\":$profile_cargo_gate_count,\"profile_gate_cache_enabled\":$profile_gate_cache_enabled,\"warm_cargo_cache_enabled\":$warm_cargo_cache_enabled,\"wall_budget_ms\":$AGENT_INNER_LOOP_BUDGET_MS}"
  )
  if [[ "$AGENT_INNER_LOOP_REQUIRE_MIN_HISTORY" == "1" ]]; then
    args+=(--require-min-history)
  fi
  python3 "${args[@]}"
}

enforce_prepush_history_budget() {
  local elapsed_ms="$1"
  local gate_count="$2"
  local profile_gate_cache_enabled=0
  local warm_cargo_cache_enabled=0
  if [[ "$HEALTH_PROFILE_GATE_CACHE" == "1" ]]; then
    profile_gate_cache_enabled=1
  fi
  if [[ "$HEALTH_WARM_CARGO_CACHE" == "1" ]]; then
    warm_cargo_cache_enabled=1
  fi
  local -a args=(
    scripts/lib/profile_runtime_budget.py
    --profile "$PROFILE"
    --kind "genesis/upgrade-plan-health-profile-v0.1"
    --report "$HEALTH_PROFILE_REPORT"
    --history "$PREPUSH_HISTORY"
    --elapsed-ms "$elapsed_ms"
    --budget-ms "$PREPUSH_WALL_BUDGET_MS"
    --min-history "$PREPUSH_MIN_HISTORY"
    --baseline-history "$PREPUSH_BASELINE_HISTORY"
    --extra-json "{\"configured_shards\":$HEALTH_SHARDS,\"profile_shards\":$PROFILE_SHARDS,\"cargo_gate_shards\":$HEALTH_CARGO_GATE_SHARDS,\"gate_count\":$gate_count,\"profile_non_cargo_gate_count\":$profile_non_cargo_gate_count,\"profile_cargo_gate_count\":$profile_cargo_gate_count,\"profile_gate_cache_enabled\":$profile_gate_cache_enabled,\"warm_cargo_cache_enabled\":$warm_cargo_cache_enabled,\"wall_budget_ms\":$PREPUSH_WALL_BUDGET_MS}"
  )
  if [[ -n "$PREPUSH_HISTORY_SCOPE_KEY" ]]; then
    args+=(--history-scope-key "$PREPUSH_HISTORY_SCOPE_KEY")
  fi
  if [[ "$PREPUSH_REQUIRE_MIN_HISTORY" == "1" ]]; then
    args+=(--require-min-history)
  fi
  python3 "${args[@]}"
}

enforce_release_full_history_budget() {
  local elapsed_ms="$1"
  local gate_count="$2"
  local profile_gate_cache_enabled=0
  local warm_cargo_cache_enabled=0
  if [[ "$HEALTH_PROFILE_GATE_CACHE" == "1" ]]; then
    profile_gate_cache_enabled=1
  fi
  if [[ "$HEALTH_WARM_CARGO_CACHE" == "1" ]]; then
    warm_cargo_cache_enabled=1
  fi
  local -a args=(
    scripts/lib/profile_runtime_budget.py
    --profile "$PROFILE"
    --kind "genesis/upgrade-plan-health-profile-v0.1"
    --report "$HEALTH_PROFILE_REPORT"
    --history "$RELEASE_FULL_HISTORY"
    --elapsed-ms "$elapsed_ms"
    --budget-ms "$RELEASE_FULL_WALL_BUDGET_MS"
    --min-history "$RELEASE_FULL_MIN_HISTORY"
    --baseline-history "$RELEASE_FULL_BASELINE_HISTORY"
    --extra-json "{\"configured_shards\":$HEALTH_SHARDS,\"profile_shards\":$PROFILE_SHARDS,\"cargo_gate_shards\":$HEALTH_CARGO_GATE_SHARDS,\"gate_count\":$gate_count,\"profile_non_cargo_gate_count\":$profile_non_cargo_gate_count,\"profile_cargo_gate_count\":$profile_cargo_gate_count,\"profile_gate_cache_enabled\":$profile_gate_cache_enabled,\"warm_cargo_cache_enabled\":$warm_cargo_cache_enabled,\"wall_budget_ms\":$RELEASE_FULL_WALL_BUDGET_MS}"
  )
  if [[ -n "$RELEASE_FULL_HISTORY_SCOPE_KEY" ]]; then
    args+=(--history-scope-key "$RELEASE_FULL_HISTORY_SCOPE_KEY")
  fi
  if [[ "$RELEASE_FULL_REQUIRE_MIN_HISTORY" == "1" ]]; then
    args+=(--require-min-history)
  fi
  python3 "${args[@]}"
}

write_health_profile_report() {
  local profile="$1"
  local configured_shards="$2"
  local gate_count="$3"
  local elapsed_ms="$4"
  local budget_ms="$5"
  local ok="$6"
  local report_path="$7"
  local history_path="$8"
  local min_history="$9"

  python3 - "$profile" "$configured_shards" "$gate_count" "$elapsed_ms" "$budget_ms" "$ok" "$report_path" "$history_path" "$min_history" <<'PY'
import json
import pathlib
import math
import sys

profile = sys.argv[1]
configured_shards = int(sys.argv[2])
gate_count = int(sys.argv[3])
elapsed_ms = int(sys.argv[4])
budget_ms_raw = sys.argv[5].strip()
ok = sys.argv[6].strip() == "1"
report_path = pathlib.Path(sys.argv[7])
history_path = pathlib.Path(sys.argv[8])
min_history = int(sys.argv[9])
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
history_path.parent.mkdir(parents=True, exist_ok=True)
history_rows = []
if history_path.is_file():
    for raw in history_path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line:
            continue
        try:
            row = json.loads(line)
        except json.JSONDecodeError:
            continue
        if (
            isinstance(row, dict)
            and row.get("kind") == "genesis/upgrade-plan-health-profile-v0.1"
            and row.get("profile") == profile
            and isinstance(row.get("elapsed_ms"), int)
            and ((budget_ms is None) or int(row.get("budget_ms", -1)) == budget_ms)
        ):
            history_rows.append(row)

history_entry = {
    "kind": "genesis/upgrade-plan-health-profile-v0.1",
    "profile": profile,
    "elapsed_ms": elapsed_ms,
    "budget_ms": budget_ms,
}
with history_path.open("a", encoding="utf-8") as fh:
    fh.write(json.dumps(history_entry, sort_keys=True) + "\n")

elapsed_samples = sorted([int(row["elapsed_ms"]) for row in history_rows] + [elapsed_ms])
p95_idx = max(0, math.ceil(0.95 * len(elapsed_samples)) - 1)
history_p95_ms = elapsed_samples[p95_idx]
history_samples = len(elapsed_samples)
history_p95_enforced = history_samples >= min_history
history_p95_ok = (not history_p95_enforced) or (budget_ms is None) or (history_p95_ms <= budget_ms)

elapsed_fail = (budget_ms is not None) and (elapsed_ms > budget_ms)
p95_fail = (budget_ms is not None) and history_p95_enforced and (history_p95_ms > budget_ms)
fail_reasons = []
if elapsed_fail:
    fail_reasons.append("elapsed-budget")
if p95_fail:
    fail_reasons.append("history-p95-budget")

total_checks = 2 if budget_ms is not None else 1
passed_checks = 0
if budget_ms is None:
    passed_checks = 1
else:
    if not elapsed_fail:
        passed_checks += 1
    if not p95_fail:
        passed_checks += 1
score_percent = round((passed_checks / total_checks) * 100.0, 2)

doc["history_file"] = str(history_path)
doc["history_samples"] = history_samples
doc["history_p95_ms"] = history_p95_ms
doc["history_p95_enforced"] = history_p95_enforced
doc["history_p95_ok"] = history_p95_ok
doc["score_percent"] = score_percent
doc["fail_reasons"] = fail_reasons
doc["ok"] = ok and (not elapsed_fail) and (not p95_fail)

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
                doc["wall_time_trend_ms"] = doc["elapsed_delta_ms"]
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
    profile:agent-inner-loop)
      CARGO_WARMUP_COMMANDS+=("cargo test -p gc_cli --test cli_smoke --no-run --quiet")
      ;;
    profile:prepush-standard)
      CARGO_WARMUP_COMMANDS+=("cargo test -p gc_cli --test cli_smoke --no-run --quiet")
      CARGO_WARMUP_COMMANDS+=("cargo test -p gc_cli --test cli_gcpm_selfhost_acceptance --no-run --quiet")
      CARGO_WARMUP_COMMANDS+=("cargo test -p gc_cli --test cli_pkg_workspace --no-run --quiet")
      ;;
    profile:release-full)
      CARGO_WARMUP_COMMANDS+=("cargo test -p gc_cli --test cli_smoke --no-run --quiet")
      CARGO_WARMUP_COMMANDS+=("cargo test -p gc_cli --test cli_gcpm_selfhost_acceptance --no-run --quiet")
      CARGO_WARMUP_COMMANDS+=("cargo test -p gc_cli --test cli_pkg_workspace --no-run --quiet")
      CARGO_WARMUP_COMMANDS+=("cargo test -p gc_effects --test gfx_gpu_bridge --no-run --quiet")
      CARGO_WARMUP_COMMANDS+=("cargo test -p gc_effects --test host_abi_surface --no-run --quiet")
      CARGO_WARMUP_COMMANDS+=("cargo test -p gc_cli --test cli_selfhost_gpu_parallel --no-run --quiet")
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

run_health_proactive_reclaim() {
  if [[ "$HEALTH_AUTO_RECLAIM" != "1" ]]; then
    echo "upgrade-plan-health: proactive reclaim disabled"
    return 0
  fi
  echo "upgrade-plan-health: proactive reclaim (max_build_kb=${HEALTH_RECLAIM_MAX_BUILD_KB}, max_age_days=${HEALTH_RECLAIM_MAX_AGE_DAYS})"
  bash scripts/reclaim_build_space.sh \
    --safe \
    --max-build-kb "$HEALTH_RECLAIM_MAX_BUILD_KB" \
    --max-age-days "$HEALTH_RECLAIM_MAX_AGE_DAYS" \
    --preserve-dir "$HEALTH_CARGO_TARGET_DIR"

  local free_kb
  free_kb="$(free_kb_root)"
  if (( free_kb < HEALTH_MIN_FREE_KB )) && [[ "$HEALTH_AUTO_AGGRESSIVE_RECLAIM_ON_LOW_DISK" == "1" ]]; then
    echo "upgrade-plan-health: low disk headroom (${free_kb}KB < ${HEALTH_MIN_FREE_KB}KB); running aggressive reclaim"
    bash scripts/reclaim_build_space.sh \
      --aggressive \
      --max-build-kb "$HEALTH_RECLAIM_MAX_BUILD_KB" \
      --max-age-days "$HEALTH_RECLAIM_MAX_AGE_DAYS"
    free_kb="$(free_kb_root)"
  fi

  if (( free_kb < HEALTH_MIN_FREE_KB )); then
    echo "upgrade-plan-health: insufficient free disk after reclaim (${free_kb}KB < ${HEALTH_MIN_FREE_KB}KB). Free disk or lower GENESIS_HEALTH_MIN_FREE_KB." >&2
    return 1
  fi

  if (( HEALTH_CARGO_GATE_SHARDS > 1 )) && (( free_kb < HEALTH_PARALLEL_CARGO_MIN_FREE_KB )); then
    echo "upgrade-plan-health: downshifting cargo gate shards ${HEALTH_CARGO_GATE_SHARDS}->1 (free_kb=${free_kb} < ${HEALTH_PARALLEL_CARGO_MIN_FREE_KB})"
    HEALTH_CARGO_GATE_SHARDS=1
  fi
}

health_profile_is_strict() {
  local profile="$1"
  [[ "$profile" == "prepush-standard" || "$profile" == "release-full" || "$profile" == "full-selfhost-cutover" ]]
}

write_health_disk_preflight_report() {
  local free_kb="$1"
  local strict_profile="$2"
  local skip_count="$3"
  local policy="$4"
  local reason="$5"
  local out_path="$6"
  local status_ok="$7"
  local skipped_csv="$8"
  python3 - "$PROFILE" "$free_kb" "$strict_profile" "$HEALTH_MIN_FREE_KB" "$HEALTH_STRICT_RUNTIME_MIN_FREE_KB" "$policy" "$reason" "$skip_count" "$out_path" "$status_ok" "$skipped_csv" <<'PY'
import datetime as dt
import json
import pathlib
import sys

profile = sys.argv[1]
free_kb = int(sys.argv[2])
strict_profile = sys.argv[3] == "1"
minimum_kb = int(sys.argv[4])
strict_runtime_min_kb = int(sys.argv[5])
policy = sys.argv[6]
reason = sys.argv[7]
skip_count = int(sys.argv[8])
out_path = pathlib.Path(sys.argv[9])
status_ok = sys.argv[10] == "1"
skipped_csv = sys.argv[11]

skipped_gates = [entry for entry in skipped_csv.split("|||") if entry]

doc = {
    "kind": "genesis/upgrade-plan-health-disk-preflight-v0.1",
    "ok": status_ok,
    "profile": profile,
    "strict_profile": strict_profile,
    "policy": policy,
    "free_kb": free_kb,
    "minimum_kb": minimum_kb,
    "strict_runtime_min_kb": strict_runtime_min_kb,
    "skip_disk_intensive_gates": skip_count > 0,
    "skip_count": skip_count,
    "reason": reason,
    "skipped_gates": skipped_gates,
    "timestamp_utc": dt.datetime.now(dt.timezone.utc).replace(microsecond=0).isoformat(),
}

out_path.parent.mkdir(parents=True, exist_ok=True)
out_path.write_text(json.dumps(doc, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY
}

usage() {
  cat <<'EOF'
Usage: scripts/check_upgrade_plan_health.sh [--profile <dev-fast|agent-inner-loop|prepush-standard|release-full|full-selfhost-cutover>]
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

if [[ "$PROFILE" != "dev-fast" && "$PROFILE" != "agent-inner-loop" && "$PROFILE" != "prepush-standard" && "$PROFILE" != "release-full" && "$PROFILE" != "full-selfhost-cutover" ]]; then
  echo "upgrade-plan-health: invalid profile '$PROFILE' (expected dev-fast|agent-inner-loop|prepush-standard|release-full|full-selfhost-cutover)" >&2
  exit 2
fi
if [[ -z "${GENESIS_HEALTH_PROFILE_REPORT:-}" && "$PROFILE" == "agent-inner-loop" ]]; then
  HEALTH_PROFILE_REPORT=".genesis/perf/upgrade_plan_health_agent_inner_loop_report.json"
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
if [[ "$HEALTH_AUTO_RECLAIM" != "0" && "$HEALTH_AUTO_RECLAIM" != "1" ]]; then
  echo "upgrade-plan-health: GENESIS_HEALTH_AUTO_RECLAIM must be 0 or 1" >&2
  exit 2
fi
if [[ ! "$HEALTH_RECLAIM_MAX_BUILD_KB" =~ ^[0-9]+$ ]]; then
  echo "upgrade-plan-health: GENESIS_HEALTH_RECLAIM_MAX_BUILD_KB must be numeric" >&2
  exit 2
fi
if [[ ! "$HEALTH_RECLAIM_MAX_AGE_DAYS" =~ ^[0-9]+$ ]]; then
  echo "upgrade-plan-health: GENESIS_HEALTH_RECLAIM_MAX_AGE_DAYS must be numeric" >&2
  exit 2
fi
if [[ ! "$HEALTH_MIN_FREE_KB" =~ ^[0-9]+$ || "$HEALTH_MIN_FREE_KB" -le 0 ]]; then
  echo "upgrade-plan-health: GENESIS_HEALTH_MIN_FREE_KB must be a positive integer" >&2
  exit 2
fi
if [[ ! "$HEALTH_PARALLEL_CARGO_MIN_FREE_KB" =~ ^[0-9]+$ || "$HEALTH_PARALLEL_CARGO_MIN_FREE_KB" -le 0 ]]; then
  echo "upgrade-plan-health: GENESIS_HEALTH_PARALLEL_CARGO_MIN_FREE_KB must be a positive integer" >&2
  exit 2
fi
if [[ "$HEALTH_AUTO_AGGRESSIVE_RECLAIM_ON_LOW_DISK" != "0" && "$HEALTH_AUTO_AGGRESSIVE_RECLAIM_ON_LOW_DISK" != "1" ]]; then
  echo "upgrade-plan-health: GENESIS_HEALTH_AUTO_AGGRESSIVE_RECLAIM_ON_LOW_DISK must be 0 or 1" >&2
  exit 2
fi
if [[ "$HEALTH_STRICT_DISK_POLICY" != "classify" && "$HEALTH_STRICT_DISK_POLICY" != "fail" ]]; then
  echo "upgrade-plan-health: GENESIS_HEALTH_STRICT_DISK_POLICY must be classify or fail" >&2
  exit 2
fi
if [[ ! "$HEALTH_STRICT_RUNTIME_MIN_FREE_KB" =~ ^[0-9]+$ || "$HEALTH_STRICT_RUNTIME_MIN_FREE_KB" -le 0 ]]; then
  echo "upgrade-plan-health: GENESIS_HEALTH_STRICT_RUNTIME_MIN_FREE_KB must be a positive integer" >&2
  exit 2
fi
if [[ "$GPU_DEVICE_CONFORMANCE" != "0" && "$GPU_DEVICE_CONFORMANCE" != "1" ]]; then
  echo "upgrade-plan-health: GENESIS_HEALTH_REQUIRE_GPU_DEVICE_CONFORMANCE must be 0 or 1" >&2
  exit 2
fi
genesis_apply_agent_gpu_profile_contract "$PROFILE" "$AGENT_AUTOMATION_CONTEXT"
if [[ -n "$AGENT_GPU_PROFILE" ]]; then
  echo "upgrade-plan-health: agent gpu profile selection=$AGENT_GPU_PROFILE"
fi
if [[ "$PROFILE" == "prepush-standard" || "$PROFILE" == "release-full" || "$PROFILE" == "full-selfhost-cutover" ]]; then
  # Strict profiles must fail closed on disk headroom checks, even in local runs.
  if [[ "$HEALTH_STRICT_DISK_POLICY" != "fail" ]]; then
    echo "upgrade-plan-health: strict profile overrides GENESIS_HEALTH_STRICT_DISK_POLICY=$HEALTH_STRICT_DISK_POLICY -> fail"
    HEALTH_STRICT_DISK_POLICY="fail"
  fi
  export GENESIS_PERF_DISK_STRICT_MODE="1"
  export GENESIS_RUNTIME_BACKEND_MATRIX_DISK_STRICT_MODE="1"
  export GENESIS_DISK_STRICT_MODE="1"
  echo "upgrade-plan-health: strict disk headroom enforcement enabled (profile=$PROFILE)"
fi
if [[ -z "$HEALTH_GPU_BACKEND_POLICY_DEFAULT" ]]; then
  case "$PROFILE" in
    prepush-standard|release-full|full-selfhost-cutover|agent-inner-loop)
      HEALTH_GPU_BACKEND_POLICY_DEFAULT="require-device"
      ;;
    *)
      HEALTH_GPU_BACKEND_POLICY_DEFAULT="allow-fallback"
      ;;
  esac
fi
if [[ "$HEALTH_GPU_BACKEND_POLICY_DEFAULT" != "require-device" && "$HEALTH_GPU_BACKEND_POLICY_DEFAULT" != "allow-fallback" && "$HEALTH_GPU_BACKEND_POLICY_DEFAULT" != "dev-allow-fallback" ]]; then
  echo "upgrade-plan-health: GENESIS_HEALTH_GPU_BACKEND_POLICY_DEFAULT must be one of require-device|allow-fallback|dev-allow-fallback" >&2
  exit 2
fi
export GENESIS_GPU_BACKEND_POLICY_DEFAULT="$HEALTH_GPU_BACKEND_POLICY_DEFAULT"
echo "upgrade-plan-health: gpu backend fallback default policy=$GENESIS_GPU_BACKEND_POLICY_DEFAULT (profile=$PROFILE)"
if [[ ! "$PREPUSH_WALL_BUDGET_MS" =~ ^[0-9]+$ || "$PREPUSH_WALL_BUDGET_MS" -le 0 ]]; then
  echo "upgrade-plan-health: GENESIS_HEALTH_PREPUSH_BUDGET_MS must be a positive integer (ms)" >&2
  exit 2
fi
if [[ ! "$PREPUSH_MIN_HISTORY" =~ ^[0-9]+$ || "$PREPUSH_MIN_HISTORY" -le 0 ]]; then
  echo "upgrade-plan-health: GENESIS_HEALTH_PREPUSH_MIN_HISTORY must be a positive integer" >&2
  exit 2
fi
if [[ "$PREPUSH_REQUIRE_MIN_HISTORY" != "0" && "$PREPUSH_REQUIRE_MIN_HISTORY" != "1" ]]; then
  echo "upgrade-plan-health: GENESIS_HEALTH_PREPUSH_REQUIRE_MIN_HISTORY must be 0 or 1" >&2
  exit 2
fi
if [[ ! "$RELEASE_FULL_WALL_BUDGET_MS" =~ ^[0-9]+$ || "$RELEASE_FULL_WALL_BUDGET_MS" -le 0 ]]; then
  echo "upgrade-plan-health: GENESIS_HEALTH_RELEASE_FULL_BUDGET_MS must be a positive integer (ms)" >&2
  exit 2
fi
if [[ ! "$RELEASE_FULL_MIN_HISTORY" =~ ^[0-9]+$ || "$RELEASE_FULL_MIN_HISTORY" -le 0 ]]; then
  echo "upgrade-plan-health: GENESIS_HEALTH_RELEASE_FULL_MIN_HISTORY must be a positive integer" >&2
  exit 2
fi
if [[ "$RELEASE_FULL_REQUIRE_MIN_HISTORY" != "0" && "$RELEASE_FULL_REQUIRE_MIN_HISTORY" != "1" ]]; then
  echo "upgrade-plan-health: GENESIS_HEALTH_RELEASE_FULL_REQUIRE_MIN_HISTORY must be 0 or 1" >&2
  exit 2
fi
if [[ ! "$DEV_FAST_PROFILE_WALL_BUDGET_MS" =~ ^[0-9]+$ || "$DEV_FAST_PROFILE_WALL_BUDGET_MS" -le 0 ]]; then
  echo "upgrade-plan-health: GENESIS_HEALTH_DEV_FAST_WALL_BUDGET_MS must be a positive integer (ms)" >&2
  exit 2
fi
if [[ ! "$AGENT_INNER_LOOP_BUDGET_MS" =~ ^[0-9]+$ || "$AGENT_INNER_LOOP_BUDGET_MS" -le 0 ]]; then
  echo "upgrade-plan-health: GENESIS_HEALTH_AGENT_INNER_LOOP_BUDGET_MS must be a positive integer (ms)" >&2
  exit 2
fi
if [[ ! "$AGENT_INNER_LOOP_MIN_HISTORY" =~ ^[0-9]+$ || "$AGENT_INNER_LOOP_MIN_HISTORY" -le 0 ]]; then
  echo "upgrade-plan-health: GENESIS_HEALTH_AGENT_INNER_LOOP_MIN_HISTORY must be a positive integer" >&2
  exit 2
fi
if [[ "$AGENT_INNER_LOOP_REQUIRE_MIN_HISTORY" != "0" && "$AGENT_INNER_LOOP_REQUIRE_MIN_HISTORY" != "1" ]]; then
  echo "upgrade-plan-health: GENESIS_HEALTH_AGENT_INNER_LOOP_REQUIRE_MIN_HISTORY must be 0 or 1" >&2
  exit 2
fi
if [[ ! "$HEALTH_PROFILE_MIN_HISTORY" =~ ^[0-9]+$ || "$HEALTH_PROFILE_MIN_HISTORY" -le 0 ]]; then
  echo "upgrade-plan-health: GENESIS_HEALTH_PROFILE_MIN_HISTORY must be a positive integer" >&2
  exit 2
fi

DEFAULT_HEALTH_SHARDS="$(default_health_shards_for_profile "$PROFILE")"
HEALTH_SHARDS="${GENESIS_HEALTH_SHARDS:-$DEFAULT_HEALTH_SHARDS}"
if [[ ! "$HEALTH_SHARDS" =~ ^[0-9]+$ || "$HEALTH_SHARDS" -le 0 ]]; then
  echo "upgrade-plan-health: GENESIS_HEALTH_SHARDS must be a positive integer" >&2
  exit 2
fi
DEFAULT_HEALTH_CARGO_GATE_SHARDS="$(default_health_cargo_gate_shards_for_profile "$PROFILE")"
if [[ -z "$HEALTH_CARGO_GATE_SHARDS" ]]; then
  HEALTH_CARGO_GATE_SHARDS="$DEFAULT_HEALTH_CARGO_GATE_SHARDS"
fi
if [[ ! "$HEALTH_CARGO_GATE_SHARDS" =~ ^[0-9]+$ || "$HEALTH_CARGO_GATE_SHARDS" -le 0 ]]; then
  echo "upgrade-plan-health: GENESIS_HEALTH_CARGO_GATE_SHARDS must be a positive integer" >&2
  exit 2
fi
if [[ "$HEALTH_PROFILE_GATE_CACHE" != "auto" && "$HEALTH_PROFILE_GATE_CACHE" != "0" && "$HEALTH_PROFILE_GATE_CACHE" != "1" ]]; then
  echo "upgrade-plan-health: GENESIS_HEALTH_PROFILE_GATE_CACHE must be auto, 0, or 1" >&2
  exit 2
fi
if [[ ! "$HEALTH_PROFILE_GATE_CACHE_TTL_SEC" =~ ^[0-9]+$ ]]; then
  echo "upgrade-plan-health: GENESIS_HEALTH_PROFILE_GATE_CACHE_TTL_SEC must be a non-negative integer" >&2
  exit 2
fi
if [[ "$HEALTH_WARM_CARGO_CACHE" == "auto" ]]; then
  if [[ "$PROFILE" == "dev-fast" || "$PROFILE" == "agent-inner-loop" ]]; then
    HEALTH_WARM_CARGO_CACHE="0"
  else
    HEALTH_WARM_CARGO_CACHE="1"
  fi
fi
if [[ "$HEALTH_PROFILE_GATE_CACHE" == "auto" ]]; then
  if [[ "$PROFILE" == "prepush-standard" ]]; then
    HEALTH_PROFILE_GATE_CACHE="1"
  else
    HEALTH_PROFILE_GATE_CACHE="0"
  fi
fi
export GENESIS_HEALTH_PROFILE_GATE_CACHE="$HEALTH_PROFILE_GATE_CACHE"
export GENESIS_HEALTH_PROFILE_GATE_CACHE_TTL_SEC="$HEALTH_PROFILE_GATE_CACHE_TTL_SEC"
echo "upgrade-plan-health: profile gate cache policy=$HEALTH_PROFILE_GATE_CACHE ttl_sec=$HEALTH_PROFILE_GATE_CACHE_TTL_SEC"

PROFILE_SHARDS="${GENESIS_HEALTH_PROFILE_SHARDS:-$HEALTH_SHARDS}"
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
run_health_proactive_reclaim

FREE_KB_AFTER_RECLAIM="$(free_kb_root)"
if health_profile_is_strict "$PROFILE" && (( FREE_KB_AFTER_RECLAIM < HEALTH_STRICT_RUNTIME_MIN_FREE_KB )); then
  HEALTH_DISK_PREFLIGHT_REASON="strict-runtime-headroom-low"
  write_health_disk_preflight_report \
    "$FREE_KB_AFTER_RECLAIM" \
    "1" \
    "0" \
    "$HEALTH_STRICT_DISK_POLICY" \
    "$HEALTH_DISK_PREFLIGHT_REASON" \
    "$HEALTH_DISK_PREFLIGHT_REPORT" \
    "0" \
    ""
  echo "upgrade-plan-health: strict runtime lanes require ${HEALTH_STRICT_RUNTIME_MIN_FREE_KB}KB free, found ${FREE_KB_AFTER_RECLAIM}KB." >&2
  echo "upgrade-plan-health: remediation: free disk and rerun, or lower strict lane reclaim caps via GENESIS_HEALTH_RECLAIM_MAX_BUILD_KB / GENESIS_HEALTH_RECLAIM_MAX_AGE_DAYS." >&2
  exit 1
else
  HEALTH_DISK_PREFLIGHT_REASON="ok"
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
  echo "upgrade-plan-health: backlog status: open checklist items = $declared_open"
  if [[ "$ENFORCE_GATES" != "1" && "$PROFILE" == "dev-fast" ]]; then
    echo "upgrade-plan-health: backlog open; running mandatory local guard gates."
    run_health_cargo_warmup "mandatory-local"
    MANDATORY_LOCAL_NON_CARGO_GATES=(
      "bash scripts/check_selfhost_boundary.sh --strict"
      "bash scripts/check_selfhost_doc_runtime_parity.sh"
      "bash scripts/check_selfhost_readiness_scorecard.sh"
      "bash scripts/check_agent_gpu_profile_contract.sh"
      "GENESIS_PLANNING_REFRESH_READINESS=0 bash scripts/check_redteam_report.sh"
      "bash scripts/check_selfhost_symbol_ownership.sh"
      "bash scripts/check_selfhost_toolchain_review_fresh.sh"
      "bash scripts/check_assurance_standards_crosswalk.sh"
      "bash scripts/check_planning_docs_fresh.sh"
      "bash scripts/check_doc_hygiene.sh"
      "bash scripts/check_doc_topology_drift.sh"
      "bash scripts/check_feature_matrix_evidence.sh"
      "bash scripts/check_write_genesiscode_skill_pack.sh"
      "bash scripts/check_write_genesiscode_skill_distribution.sh"
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
      "$HEALTH_PROFILE_REPORT" \
      "$HEALTH_PROFILE_HISTORY" \
      "$HEALTH_PROFILE_MIN_HISTORY"
    echo "upgrade-plan-health: mandatory-local elapsed_ms=${elapsed_ms} gate_count=${gate_count}"
    if (( mandatory_ok == 0 )); then
      echo "upgrade-plan-health: dev-fast mandatory-local wall-time exceeded budget (${elapsed_ms}ms > ${DEV_FAST_PROFILE_WALL_BUDGET_MS}ms)" >&2
      exit 1
    fi
    echo "upgrade-plan-health: ok"
    exit 0
  fi
  if [[ "$ENFORCE_GATES" != "1" ]]; then
    echo "upgrade-plan-health: backlog open; enforcing full profile gates for profile=$PROFILE (mandatory-local is dev-fast-only)."
  else
    echo "upgrade-plan-health: code health gates enforced despite backlog (profile=$PROFILE)"
  fi
else
  echo "upgrade-plan-health: backlog status: open checklist items = 0"
  echo "upgrade-plan-health: code health gates enforced (profile=$PROFILE)"
fi

COMMON_GATES=(
  "bash scripts/check_selfhost_boundary.sh --strict"
  "bash scripts/check_host_abi_conformance.sh"
  "bash scripts/check_agent_gpu_profile_contract.sh"
  "bash scripts/check_runner_high_level_op_guard.sh"
  "bash scripts/check_prelude_capability_coverage.sh"
  "bash scripts/check_foundation_stdlib_conformance.sh"
  "bash scripts/check_capability_indices.sh"
  "bash scripts/check_capability_coverage_audit.sh"
  "bash scripts/check_host_api_evolution_contracts.sh"
  "bash scripts/check_gcpm_operation_contract_pack.sh"
  "bash scripts/check_assurance_profile_packs.sh"
  "bash scripts/check_assurance_standards_crosswalk.sh"
  "bash scripts/check_tool_qualification_lineage.sh"
  "bash scripts/check_cargo_target_dir_policy.sh"
  "bash scripts/check_selfhost_symbol_ownership.sh"
  "bash scripts/check_agent_authoring_bundle.sh"
  "bash scripts/check_genesiscode_authoring_skill.sh"
  "bash scripts/check_domain_kit_workflows.sh"
  "bash scripts/check_domain_starter_registry_bootstrap.sh"
  "bash scripts/check_selfhost_refactor_guard.sh"
  "bash scripts/check_selfhost_artifact_fresh.sh"
  "bash scripts/check_selfhost_toolchain_review_fresh.sh"
  "bash scripts/check_selfhost_dashboard_fresh.sh"
  "bash scripts/check_selfhost_readiness_scorecard.sh"
  "bash scripts/check_selfhost_doc_runtime_parity.sh"
  "GENESIS_PLANNING_REFRESH_READINESS=0 bash scripts/check_redteam_report.sh"
  "GENESIS_PLANNING_REFRESH_READINESS=0 bash scripts/check_feature_matrix_gap_hygiene.sh"
  "bash scripts/check_feature_matrix_evidence.sh"
  "bash scripts/check_planning_docs_fresh.sh"
  "bash scripts/check_doc_hygiene.sh"
  "bash scripts/check_doc_topology_drift.sh"
  "bash scripts/check_doc_complexity_budget.sh"
  "bash scripts/check_write_genesiscode_skill_pack.sh"
  "bash scripts/check_write_genesiscode_skill_guide.sh"
  "bash scripts/check_write_genesiscode_skill_distribution.sh"
  "bash scripts/check_task_concurrency_stress.sh"
  "bash scripts/check_host_bridge_fault_injection.sh"
  "bash scripts/check_no_user_panics.sh"
  "bash scripts/check_rust_engine_compat.sh"
  "bash scripts/check_kernel_tcb_contract.sh"
  "bash scripts/check_vcs_selfhost_contract.sh"
  "bash scripts/check_no_production_rust_frontend_refs.sh"
  "GENESIS_PRODUCTION_CLI_HELP_SURFACE_INCLUDE_PARITY=0 bash scripts/check_production_cli_help_surface.sh"
  "bash scripts/check_cli_diagnostics_contract.sh"
  "bash scripts/check_fuzz_differential_hardening.sh"
  "bash scripts/check_test_execution_profile_matrix.sh"
  "bash scripts/check_gpu_conformance_lane_matrix.sh"
  "bash scripts/check_gpu_stack_decoupling.sh"
  "bash scripts/check_webxr_browser_conformance_lane.sh"
  "bash scripts/check_gc_source_size_budget.sh"
  "bash scripts/check_source_size_budget.sh"
  "bash scripts/check_source_decomposition_progress.sh"
  "bash scripts/check_selfhost_gc_migration_plan.sh"
  "bash scripts/check_test_size_budget.sh"
)

if [[ "$PROFILE" == "agent-inner-loop" ]]; then
  # Inner-loop profile intentionally runs a narrow, high-signal deterministic contract set.
  # This avoids repeating heavyweight process startups while preserving selfhost/agent safety invariants.
  COMMON_GATES=(
    "bash scripts/check_selfhost_boundary.sh --strict"
    "bash scripts/check_selfhost_doc_runtime_parity.sh"
    "bash scripts/check_selfhost_readiness_scorecard.sh"
    "bash scripts/check_feature_matrix_gap_hygiene.sh"
    "bash scripts/check_redteam_report.sh"
    "bash scripts/check_doc_complexity_budget.sh"
    "bash scripts/check_source_decomposition_progress.sh"
    "bash scripts/check_selfhost_gc_migration_plan.sh"
    "bash scripts/check_tool_qualification_lineage.sh"
    "bash scripts/check_write_genesiscode_skill_guide.sh"
    "bash scripts/check_cli_diagnostics_contract.sh"
    "bash scripts/check_kernel_tcb_contract.sh"
    "bash scripts/check_vcs_selfhost_contract.sh"
    "bash scripts/check_no_production_rust_frontend_refs.sh"
  )
fi

PROFILE_GATES=()
case "$PROFILE" in
  dev-fast)
    PROFILE_GATES+=("cargo test -p gc_cli --test cli_smoke --quiet")
    PROFILE_GATES+=(
      "bash scripts/test_changed_fast.sh --base HEAD --runner auto --budget-ms ${DEV_FAST_BUDGET_MS} --min-history 1 --report .genesis/perf/upgrade_plan_dev_fast_metrics.json --history .genesis/perf/upgrade_plan_dev_fast_history.jsonl"
    )
    ;;
  agent-inner-loop)
    PROFILE_GATES+=("cargo test -p gc_cli --test cli_smoke --quiet")
    PROFILE_GATES+=(
      "bash scripts/test_changed_fast.sh --base HEAD --runner auto --budget-ms ${DEV_FAST_BUDGET_MS} --min-history 1 --report .genesis/perf/agent_inner_loop_changed_fast_metrics.json --history .genesis/perf/agent_inner_loop_changed_fast_history.jsonl"
    )
    PROFILE_GATES+=("GENESIS_FULL_SELFHOST_CUTOVER_REFRESH=0 bash scripts/check_full_selfhost_cutover_profile.sh")
    ;;
  prepush-standard)
    PROFILE_GATES+=("cargo clippy --workspace --all-targets -- -D warnings")
    PROFILE_GATES+=("cargo test -p gc_cli --test cli_smoke --quiet")
    PROFILE_GATES+=("cargo test -p gc_cli --test cli_gcpm_selfhost_acceptance --quiet")
    PROFILE_GATES+=("cargo test -p gc_cli --test cli_pkg_workspace gcpm_build_target_is_reproducible_and_emits_provenance_bundle --quiet")
    PROFILE_GATES+=("cargo test -p gc_cli --test cli_pkg_workspace gcpm_build_supports_mobile_and_edge_target_contracts --quiet")
    PROFILE_GATES+=("bash scripts/check_gcpm_target_runtime_pipelines.sh")
    PROFILE_GATES+=(
      "GENESIS_RUNTIME_BACKEND_MATRIX_MIN_FREE_KB=1048576 GENESIS_RUNTIME_BACKEND_MATRIX_CLEAN_TARGET_DIR=0 GENESIS_RUNTIME_BACKEND_MATRIX_CARGO_PROFILE_DEV_DEBUG=0 GENESIS_RUNTIME_BACKEND_MATRIX_CARGO_INCREMENTAL=1 GENESIS_RUNTIME_BACKEND_MATRIX_STAGE_AUTO_RECLAIM=0 bash scripts/check_runtime_backend_feature_matrix.sh"
    )
    PROFILE_GATES+=("bash scripts/check_bootstrap_retirement_gate.sh")
    PROFILE_GATES+=("GENESIS_AGENT_GAUNTLET_PROFILE=prepush-standard GENESIS_AGENT_GAUNTLET_REQUIRE_GPU_DEVICE_BACKEND=1 GENESIS_AGENT_GAUNTLET_REGRESSION_PERCENT=60 GENESIS_AGENT_GAUNTLET_REGRESSION_SLACK_MS=3000 bash scripts/check_agent_reference_workflows.sh")
    PROFILE_GATES+=("GENESIS_GPU_XR_REQUIRE_WEBXR_RUNTIME_EVIDENCE=1 bash scripts/check_gpu_xr_productization_kits.sh")
    PROFILE_GATES+=("bash scripts/check_slo_report_contracts.sh")
    PROFILE_GATES+=(
      "GENESIS_AGENT_GENERATIVE_PRIMARY_REPORT=.genesis/perf/agent_capability_gauntlet_report.json GENESIS_AGENT_GENERATIVE_REQUIRE_SECONDARY=0 bash scripts/check_agent_generative_workloads.sh"
    )
    PROFILE_GATES+=("GENESIS_WRITE_SKILL_CONFORMANCE_PROFILE=prepush-standard bash scripts/check_write_genesiscode_skill_conformance.sh")
    PROFILE_GATES+=("GENESIS_WRITE_SKILL_DIST_VERIFY_RUNTIME=1 GENESIS_WRITE_SKILL_DIST_CONFORMANCE_AUTO_RUN=0 bash scripts/check_write_genesiscode_skill_distribution.sh")
    PROFILE_GATES+=("GENESIS_BUDGET_WARMUPS=0 GENESIS_BUDGET_REPEATS=1 bash scripts/check_perf_budgets.sh")
    PROFILE_GATES+=("GENESIS_AI_ITERATION_SLO_SAMPLES_INCREMENTAL_WARM=1 GENESIS_AI_ITERATION_SLO_SAMPLES_CHANGED_FAST=1 GENESIS_AI_ITERATION_SLO_SAMPLES_CORE_SUITE=1 GENESIS_AI_ITERATION_SLO_SAMPLES_GCPM_LOCK=1 GENESIS_AI_ITERATION_SLO_SAMPLES_GCPM_ENV=1 GENESIS_AI_ITERATION_SLO_WARMUP_GCPM_LOCK=0 GENESIS_AI_ITERATION_SLO_WARMUP_GCPM_ENV=0 GENESIS_AI_ITERATION_SLO_STABILIZE_RETRIES_GCPM_LOCK=0 GENESIS_AI_ITERATION_SLO_STABILIZE_RETRIES_GCPM_ENV=0 bash scripts/check_ai_iteration_slo.sh")
    PROFILE_GATES+=("bash scripts/check_runtime_microbench_budgets.sh")
    PROFILE_GATES+=("bash scripts/check_gpu_compute_runtime_profile.sh")
    PROFILE_GATES+=("bash scripts/check_gfx_runtime_profile.sh")
    PROFILE_GATES+=("bash scripts/check_gpu_gfx_headroom_conformance.sh")
    ;;
  release-full)
    PROFILE_GATES+=("cargo clippy --workspace --all-targets -- -D warnings")
    PROFILE_GATES+=("cargo test -p gc_cli --test cli_smoke --quiet")
    PROFILE_GATES+=("cargo test -p gc_cli --test cli_gcpm_selfhost_acceptance --quiet")
    PROFILE_GATES+=("cargo test -p gc_cli --test cli_pkg_workspace gcpm_build_target_is_reproducible_and_emits_provenance_bundle --quiet")
    PROFILE_GATES+=("cargo test -p gc_cli --test cli_pkg_workspace gcpm_build_supports_mobile_and_edge_target_contracts --quiet")
    PROFILE_GATES+=("bash scripts/check_gcpm_target_runtime_pipelines.sh")
    PROFILE_GATES+=(
      "GENESIS_RUNTIME_BACKEND_MATRIX_CARGO_TARGET_DIR=$ROOT_DIR/.genesis/build/runtime_backend_feature_matrix GENESIS_RUNTIME_BACKEND_MATRIX_CLEAN_TARGET_DIR=1 GENESIS_RUNTIME_BACKEND_MATRIX_CARGO_PROFILE_DEV_DEBUG=0 GENESIS_RUNTIME_BACKEND_MATRIX_CARGO_INCREMENTAL=0 bash scripts/check_runtime_backend_feature_matrix.sh"
    )
    PROFILE_GATES+=("bash scripts/check_bootstrap_retirement_gate.sh")
    PROFILE_GATES+=("GENESIS_FULL_SELFHOST_CUTOVER_REFRESH=0 bash scripts/check_full_selfhost_cutover_profile.sh")
    PROFILE_GATES+=(
      "GENESIS_AGENT_GAUNTLET_PROFILE=release-full GENESIS_AGENT_GAUNTLET_REQUIRE_GPU_DEVICE_BACKEND=1 bash scripts/check_agent_reference_workflows.sh"
    )
    PROFILE_GATES+=("GENESIS_GPU_XR_REQUIRE_WEBXR_RUNTIME_EVIDENCE=1 bash scripts/check_gpu_xr_productization_kits.sh")
    PROFILE_GATES+=("bash scripts/check_slo_report_contracts.sh")
    PROFILE_GATES+=("bash scripts/check_agent_scenario_perf.sh")
    PROFILE_GATES+=("GENESIS_WRITE_SKILL_CONFORMANCE_PROFILE=release-full bash scripts/check_write_genesiscode_skill_conformance.sh")
    PROFILE_GATES+=("GENESIS_WRITE_SKILL_DIST_VERIFY_RUNTIME=1 GENESIS_WRITE_SKILL_DIST_CONFORMANCE_AUTO_RUN=0 bash scripts/check_write_genesiscode_skill_distribution.sh")
    PROFILE_GATES+=("GENESIS_AGENT_PARITY_GAUNTLET_PROFILE=prepush-standard bash scripts/check_agent_workflow_runtime_parity.sh")
    PROFILE_GATES+=(
      "GENESIS_AGENT_GENERATIVE_PRIMARY_REPORT=.genesis/perf/agent_capability_gauntlet_native_report.json GENESIS_AGENT_GENERATIVE_SECONDARY_REPORT=.genesis/perf/agent_capability_gauntlet_wasi_report.json GENESIS_AGENT_GENERATIVE_REQUIRE_SECONDARY=1 bash scripts/check_agent_generative_workloads.sh"
    )
    PROFILE_GATES+=(
      "GENESIS_PRODUCTION_CLI_HELP_SURFACE_INCLUDE_PARITY=1 GENESIS_PRODUCTION_CLI_HELP_SURFACE_REPORT=.genesis/perf/production_cli_help_surface_parity_report.json GENESIS_PRODUCTION_CLI_HELP_SURFACE_HISTORY=.genesis/perf/production_cli_help_surface_parity_history.jsonl GENESIS_PRODUCTION_CLI_HELP_SURFACE_HISTORY_SCOPE_KEY=production-plus-parity-v1 bash scripts/check_production_cli_help_surface.sh"
    )
    PROFILE_GATES+=("GENESIS_SLO_REQUIRE_PARITY_REPORT=1 bash scripts/check_slo_report_contracts.sh")
    PROFILE_GATES+=(
      "GENESIS_TASK_STRESS_RUNS=6 GENESIS_TASK_STRESS_ITERATIONS=8 GENESIS_TASK_STRESS_MAX_FAILURE_RATE_PCT=0 GENESIS_TASK_STRESS_SUITE_BUDGET_MS=420000 bash scripts/check_task_concurrency_stress.sh"
    )
    PROFILE_GATES+=(
      "GENESIS_HOST_BRIDGE_FAULT_RUNS=6 GENESIS_HOST_BRIDGE_FAULT_MAX_FAILURE_RATE_PCT=0 GENESIS_HOST_BRIDGE_FAULT_BUDGET_MS=300000 bash scripts/check_host_bridge_fault_injection.sh"
    )
    PROFILE_GATES+=("bash scripts/check_perf_budgets.sh")
    PROFILE_GATES+=("bash scripts/check_ai_iteration_slo.sh")
    PROFILE_GATES+=("bash scripts/check_large_workspace_agent_perf.sh")
    PROFILE_GATES+=("bash scripts/check_ai_stress_suite.sh")
    PROFILE_GATES+=("bash scripts/check_hot_path_budgets.sh")
    PROFILE_GATES+=("bash scripts/check_runtime_microbench_budgets.sh")
    PROFILE_GATES+=("bash scripts/check_gpu_compute_runtime_profile.sh")
    PROFILE_GATES+=("bash scripts/check_gfx_runtime_profile.sh")
    PROFILE_GATES+=("bash scripts/check_gpu_gfx_headroom_conformance.sh")
    PROFILE_GATES+=("bash scripts/check_wasm_production_surface.sh")
    ;;
  full-selfhost-cutover)
    PROFILE_GATES+=("GENESIS_FULL_SELFHOST_CUTOVER_REFRESH=1 bash scripts/check_full_selfhost_cutover_profile.sh")
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

write_health_disk_preflight_report \
  "$FREE_KB_AFTER_RECLAIM" \
  "$(health_profile_is_strict "$PROFILE" && echo 1 || echo 0)" \
  "0" \
  "$HEALTH_STRICT_DISK_POLICY" \
  "$HEALTH_DISK_PREFLIGHT_REASON" \
  "$HEALTH_DISK_PREFLIGHT_REPORT" \
  "1" \
  ""

if [[ -n "$TEST_GATE_OVERRIDE" ]]; then
  COMMON_GATES=("$TEST_GATE_OVERRIDE")
  PROFILE_GATES=()
fi

apply_profile_gate_cache_policy

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
elif [[ "$PROFILE" == "agent-inner-loop" ]]; then
  profile_budget="$AGENT_INNER_LOOP_BUDGET_MS"
  if (( elapsed_ms > AGENT_INNER_LOOP_BUDGET_MS )); then
    profile_ok=0
  fi
elif [[ "$PROFILE" == "prepush-standard" ]]; then
  profile_budget="$PREPUSH_WALL_BUDGET_MS"
  if (( elapsed_ms > PREPUSH_WALL_BUDGET_MS )); then
    profile_ok=0
  fi
elif [[ "$PROFILE" == "release-full" ]]; then
  profile_budget="$RELEASE_FULL_WALL_BUDGET_MS"
  if (( elapsed_ms > RELEASE_FULL_WALL_BUDGET_MS )); then
    profile_ok=0
  fi
fi

if [[ "$PROFILE" == "agent-inner-loop" ]]; then
  enforce_inner_loop_history_budget "$elapsed_ms" "$gate_count"
elif [[ "$PROFILE" == "prepush-standard" ]]; then
  enforce_prepush_history_budget "$elapsed_ms" "$gate_count"
elif [[ "$PROFILE" == "release-full" ]]; then
  enforce_release_full_history_budget "$elapsed_ms" "$gate_count"
else
  write_health_profile_report \
    "$PROFILE" \
    "$HEALTH_SHARDS" \
    "$gate_count" \
    "$elapsed_ms" \
    "$profile_budget" \
    "$profile_ok" \
    "$HEALTH_PROFILE_REPORT" \
    "$HEALTH_PROFILE_HISTORY" \
    "$HEALTH_PROFILE_MIN_HISTORY"
fi

if (( profile_ok == 0 )); then
  if [[ "$PROFILE" == "dev-fast" ]]; then
    echo "upgrade-plan-health: dev-fast wall-time exceeded budget (${elapsed_ms}ms > ${DEV_FAST_PROFILE_WALL_BUDGET_MS}ms)" >&2
  elif [[ "$PROFILE" == "agent-inner-loop" ]]; then
    echo "upgrade-plan-health: agent-inner-loop wall-time exceeded budget (${elapsed_ms}ms > ${AGENT_INNER_LOOP_BUDGET_MS}ms)" >&2
  elif [[ "$PROFILE" == "prepush-standard" ]]; then
    echo "upgrade-plan-health: prepush wall-time exceeded budget (${elapsed_ms}ms > ${PREPUSH_WALL_BUDGET_MS}ms)" >&2
  elif [[ "$PROFILE" == "release-full" ]]; then
    echo "upgrade-plan-health: release-full wall-time exceeded budget (${elapsed_ms}ms > ${RELEASE_FULL_WALL_BUDGET_MS}ms)" >&2
  else
    echo "upgrade-plan-health: profile wall-time exceeded budget (${elapsed_ms}ms > ${profile_budget}ms)" >&2
  fi
  exit 1
fi

echo "upgrade-plan-health: elapsed_ms=${elapsed_ms} gate_count=${gate_count} shards=${HEALTH_SHARDS}"
echo "upgrade-plan-health: ok"
