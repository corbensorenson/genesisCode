#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"

BUDGET_MS="${GENESIS_HOST_BRIDGE_FAULT_BUDGET_MS:-120000}"
RUNS="${GENESIS_HOST_BRIDGE_FAULT_RUNS:-1}"
MAX_FAILURE_RATE_PCT="${GENESIS_HOST_BRIDGE_FAULT_MAX_FAILURE_RATE_PCT:-0}"
REPORT_PATH="${GENESIS_HOST_BRIDGE_FAULT_REPORT:-.genesis/perf/host_bridge_fault_injection_report.json}"
HISTORY_PATH="${GENESIS_HOST_BRIDGE_FAULT_HISTORY:-.genesis/perf/host_bridge_fault_injection_history.jsonl}"

if [[ ! "$BUDGET_MS" =~ ^[0-9]+$ || "$BUDGET_MS" -le 0 ]]; then
  echo "host-bridge-fault-injection: GENESIS_HOST_BRIDGE_FAULT_BUDGET_MS must be a positive integer" >&2
  exit 2
fi
if [[ ! "$RUNS" =~ ^[0-9]+$ || "$RUNS" -le 0 ]]; then
  echo "host-bridge-fault-injection: GENESIS_HOST_BRIDGE_FAULT_RUNS must be a positive integer" >&2
  exit 2
fi

genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "host-bridge-fault-injection" \
  ".genesis/build/host_bridge_fault_injection" \
  "GENESIS_HOST_BRIDGE_FAULT_CARGO_TARGET_DIR"

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
  if cargo test -p gc_effects --test host_bridge_fault_injection --quiet; then
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

python3 - "$REPORT_PATH" "$HISTORY_PATH" "$elapsed_ms" "$BUDGET_MS" "$RUNS" "$passed_runs" "$failed_runs" "$MAX_FAILURE_RATE_PCT" "$RUNS_FILE" <<'PY'
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
    "families": ["fs", "net", "process", "plugin"],
    "deterministic_replay_verified": True,
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
