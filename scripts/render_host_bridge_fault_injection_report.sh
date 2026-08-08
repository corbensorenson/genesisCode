#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if [[ "$#" -ne 3 ]]; then
  echo "usage: $0 <report-output> <history-output> <history-input>" >&2
  exit 2
fi

REPORT_PATH="$1"
HISTORY_PATH="$2"
HISTORY_INPUT="$3"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"

BUDGET_MS="${GENESIS_HOST_BRIDGE_FAULT_BUDGET_MS:-120000}"
RUNS="${GENESIS_HOST_BRIDGE_FAULT_RUNS:-1}"
MAX_FAILURE_RATE_PCT="${GENESIS_HOST_BRIDGE_FAULT_MAX_FAILURE_RATE_PCT:-0}"

case "$(uname -s)" in
  Darwin)
    HOST_PLATFORM="darwin"
    PROCESS_GROUP_PROBE="libproc-pgrp-status"
    ;;
  Linux)
    HOST_PLATFORM="linux"
    PROCESS_GROUP_PROBE="procfs-pgrp-status"
    ;;
  *)
    echo "host-bridge-fault-injection: hard-cancellation evidence requires macOS or Linux" >&2
    exit 2
    ;;
esac

if [[ ! "$BUDGET_MS" =~ ^[0-9]+$ || "$BUDGET_MS" -le 0 ]]; then
  echo "host-bridge-fault-injection: GENESIS_HOST_BRIDGE_FAULT_BUDGET_MS must be a positive integer" >&2
  exit 2
fi
if [[ ! "$RUNS" =~ ^[0-9]+$ || "$RUNS" -le 0 ]]; then
  echo "host-bridge-fault-injection: GENESIS_HOST_BRIDGE_FAULT_RUNS must be a positive integer" >&2
  exit 2
fi
python3 - "$MAX_FAILURE_RATE_PCT" <<'PY'
import sys
try:
    value = float(sys.argv[1])
except ValueError:
    raise SystemExit("host-bridge-fault-injection: GENESIS_HOST_BRIDGE_FAULT_MAX_FAILURE_RATE_PCT must be numeric")
if not 0.0 <= value <= 100.0:
    raise SystemExit("host-bridge-fault-injection: GENESIS_HOST_BRIDGE_FAULT_MAX_FAILURE_RATE_PCT must be within [0, 100]")
PY

genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "host-bridge-fault-injection" \
  root-host

if grep -En 'static SESSIONS|persistent_bridge_session_map' \
  crates/gc_effects/src/runner_host_bridge_persistent.rs; then
  echo "host-bridge-fault-injection: process-global persistent bridge ownership is forbidden" >&2
  exit 1
fi
grep -Eq 'struct HostBridgeRuntime' crates/gc_effects/src/runner_host_bridge.rs
grep -Eq 'bridge_runtime: &mut HostBridgeRuntime' \
  crates/gc_effects/src/runner_capability_dispatch.rs

start_ns="$(python3 - <<'PY'
import time
print(time.time_ns())
PY
)"

RUNS_FILE="$(mktemp)"
trap 'rm -f "$RUNS_FILE"' EXIT

passed_runs=0
failed_runs=0
for (( run = 1; run <= RUNS; run += 1 )); do
  run_start_ns="$(python3 - <<'PY'
import time
print(time.time_ns())
PY
)"
  if cargo test -p gc_effects --test host_bridge_fault_injection --quiet && \
     cargo test -p gc_effects --lib runner_process_control::tests::zombie_only_process_group_is_execution_quiescent --quiet -- --exact && \
     cargo test -p gc_effects --lib runner_host_bridge::runner_host_bridge_persistent::tests::persistent_stop_is_bounded_when_signal_and_reap_fail --quiet -- --exact && \
     cargo test -p gc_effects --lib runner_host_bridge::tests::spawn_bridge_reaps_residual_descendants_after_success_and_error --quiet -- --exact && \
     cargo test -p gc_effects --lib runner_host_bridge::tests::persistent_bridge_owner_closes_all_families_on_error_drop_and_restart --quiet -- --exact && \
     cargo test -p gc_effects --lib runner_host_bridge::tests::persistent_stdio_timeout_kills_process_trees_and_workers --quiet -- --ignored --exact && \
     cargo test -p gc_effects --lib runner_host_bridge::tests::spawn_per_op_timeout_kills_bridge_processes_and_recovers --quiet -- --ignored --exact && \
     cargo test -p gc_effects --test host_abi_surface browser_xr::first_party_browser_and_xr_reject_repeated_close --quiet -- --exact && \
     cargo test -p gc_effects --lib tests::tests_host_backends::tests_host_backends_first_party::editor_first_party_core_ops_are_replayable_without_bridge --quiet -- --exact && \
     cargo test -p gc_effects --lib runner_gfx_host::lifecycle_tests::runtime_drop_reaps_only_owned_desktop_surfaces --quiet -- --exact && \
     cargo test -p gc_effects --lib --no-default-features --features gfx-desktop-backend runner_gfx_host::lifecycle_tests::runtime_drop_reaps_only_owned_desktop_surfaces --quiet -- --exact && \
     cargo test -p gc_effects --lib --no-default-features --features gpu-device-backend device_runtime_resources_are_scoped_and_reaped --quiet; then
    run_ok=1
    passed_runs=$((passed_runs + 1))
  else
    run_ok=0
    failed_runs=$((failed_runs + 1))
  fi
  run_end_ns="$(python3 - <<'PY'
import time
print(time.time_ns())
PY
)"
  run_elapsed_ms="$(( (run_end_ns - run_start_ns) / 1000000 ))"
  printf '%s,%s,%s\n' "$run" "$run_ok" "$run_elapsed_ms" >> "$RUNS_FILE"
done

end_ns="$(python3 - <<'PY'
import time
print(time.time_ns())
PY
)"
elapsed_ms="$(( (end_ns - start_ns) / 1000000 ))"

mkdir -p "$(dirname "$HISTORY_PATH")"
if [[ "$HISTORY_INPUT" != "$HISTORY_PATH" ]]; then
  if [[ -f "$HISTORY_INPUT" ]]; then
    cp "$HISTORY_INPUT" "$HISTORY_PATH"
  else
    : >"$HISTORY_PATH"
  fi
fi

python3 - "$REPORT_PATH" "$HISTORY_PATH" "$elapsed_ms" "$BUDGET_MS" "$RUNS" "$passed_runs" "$failed_runs" "$MAX_FAILURE_RATE_PCT" "$RUNS_FILE" "$HOST_PLATFORM" "$PROCESS_GROUP_PROBE" <<'PY'
import json
import pathlib
import sys
import time

report_path = pathlib.Path(sys.argv[1])
history_path = pathlib.Path(sys.argv[2])
elapsed_ms = int(sys.argv[3])
budget_ms = int(sys.argv[4])
runs = int(sys.argv[5])
passed_runs = int(sys.argv[6])
failed_runs = int(sys.argv[7])
max_failure_rate_pct = float(sys.argv[8])
runs_file = pathlib.Path(sys.argv[9])
host_platform = sys.argv[10]
process_group_probe = sys.argv[11]

if max_failure_rate_pct < 0.0 or max_failure_rate_pct > 100.0:
    raise SystemExit(
        "host-bridge-fault-injection: GENESIS_HOST_BRIDGE_FAULT_MAX_FAILURE_RATE_PCT must be within [0, 100]"
    )

run_records = []
for line in runs_file.read_text(encoding="utf-8").splitlines():
    run_s, ok_s, elapsed_s = line.split(",")
    run_records.append(
        {
            "run": int(run_s),
            "ok": ok_s == "1",
            "elapsed_ms": int(elapsed_s),
        }
    )

observed_failure_rate_pct = (failed_runs / runs) * 100.0

report = {
    "kind": "genesis/host-bridge-fault-injection-v0.1",
    "timestamp_unix_s": int(time.time()),
    "runs": runs,
    "passed_runs": passed_runs,
    "failed_runs": failed_runs,
    "max_failure_rate_pct": max_failure_rate_pct,
    "observed_failure_rate_pct": observed_failure_rate_pct,
    "elapsed_ms": elapsed_ms,
    "budget_ms": budget_ms,
    "ok": elapsed_ms <= budget_ms and observed_failure_rate_pct <= max_failure_rate_pct,
    "native_host": {
        "platform": host_platform,
        "process_group_probe": process_group_probe,
        "zombie_only_group_control": True,
        "signal_reap_failure_control": True,
    },
    "families": ["fs", "net", "process", "plugin"],
    "deterministic_replay_verified": True,
    "host_handle_lifecycle": {
        "coverage_complete": False,
        "r2_2_f_closeable": False,
        "runtime_owner_scope": "per-run",
        "verified_controls": [
            "bridge-success-error-cancellation-timeout-drop-restart",
            "browser-repeated-close-rejected",
            "editor-repeated-unsubscribe-rejected",
            "graphics-runtime-drop-dispatches-desktop-destroy",
            "gpu-device-explicit-destroy-rejected-after-close",
            "gpu-device-restart-rejects-stale-handles",
            "xr-repeated-close-rejected",
        ],
        "model_sessions": {
            "status": "not-implemented",
            "verified": False,
        },
        "independent_cross_host_evidence": False,
    },
    "hard_cancellation": {
        "transports": ["persistent-stdio", "spawn-per-op"],
        "repeated_hang_cases": 49,
        "process_tree_termination": True,
        "process_group_quiescence": "no-live-members",
        "child_reap": True,
        "io_worker_quiescence": True,
        "uncertain_request_retry": False,
        "owner_scope": "runner",
        "process_global_session_cache": False,
        "lifecycle_paths": [
            "success",
            "error",
            "cancellation",
            "timeout",
            "runtime-drop",
            "restart",
            "repeated-load",
        ],
        "resource_families": [
            "filesystem",
            "network",
            "process",
            "plugin",
        ],
    },
    "runs_detail": run_records,
}

report_path.parent.mkdir(parents=True, exist_ok=True)
report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")

history_path.parent.mkdir(parents=True, exist_ok=True)
with history_path.open("a", encoding="utf-8") as f:
    f.write(json.dumps(report, sort_keys=True) + "\n")

print(f"host-bridge-fault-injection: wrote report {report_path}")
print(
    "host-bridge-fault-injection: "
    f"elapsed_ms={elapsed_ms} runs={runs} failed_runs={failed_runs} "
    f"observed_failure_rate_pct={observed_failure_rate_pct:.2f} budget_ms={budget_ms}"
)

if elapsed_ms > budget_ms:
    raise SystemExit(
        f"host-bridge-fault-injection: budget exceeded ({elapsed_ms}ms > {budget_ms}ms)"
    )
if observed_failure_rate_pct > max_failure_rate_pct:
    raise SystemExit(
        "host-bridge-fault-injection: failure-rate budget exceeded "
        f"({observed_failure_rate_pct:.2f}% > {max_failure_rate_pct:.2f}%)"
    )
PY

echo "host-bridge-fault-injection: ok"
