#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

ITERATIONS="${GENESIS_TASK_STRESS_ITERATIONS:-2}"
TEST_BUDGET_MS="${GENESIS_TASK_STRESS_BUDGET_MS:-75000}"
SUITE_BUDGET_MS="${GENESIS_TASK_STRESS_SUITE_BUDGET_MS:-120000}"
REPORT_PATH="${GENESIS_TASK_STRESS_REPORT:-.genesis/perf/task_concurrency_stress_report.json}"
HISTORY_PATH="${GENESIS_TASK_STRESS_HISTORY:-.genesis/perf/task_concurrency_stress_history.jsonl}"

if [[ ! "$ITERATIONS" =~ ^[0-9]+$ || "$ITERATIONS" -le 0 ]]; then
  echo "task-concurrency-stress: GENESIS_TASK_STRESS_ITERATIONS must be a positive integer" >&2
  exit 2
fi
if [[ ! "$TEST_BUDGET_MS" =~ ^[0-9]+$ || "$TEST_BUDGET_MS" -le 0 ]]; then
  echo "task-concurrency-stress: GENESIS_TASK_STRESS_BUDGET_MS must be a positive integer" >&2
  exit 2
fi
if [[ ! "$SUITE_BUDGET_MS" =~ ^[0-9]+$ || "$SUITE_BUDGET_MS" -le 0 ]]; then
  echo "task-concurrency-stress: GENESIS_TASK_STRESS_SUITE_BUDGET_MS must be a positive integer" >&2
  exit 2
fi

start_ns="$(python3 - <<'PY'
import time
print(time.time_ns())
PY
)"

GENESIS_TASK_STRESS_ITERATIONS="$ITERATIONS" \
GENESIS_TASK_STRESS_BUDGET_MS="$TEST_BUDGET_MS" \
cargo test -p gc_effects --test task_concurrency_stress --quiet

end_ns="$(python3 - <<'PY'
import time
print(time.time_ns())
PY
)"
elapsed_ms="$(( (end_ns - start_ns) / 1000000 ))"

python3 - "$REPORT_PATH" "$HISTORY_PATH" "$elapsed_ms" "$ITERATIONS" "$TEST_BUDGET_MS" "$SUITE_BUDGET_MS" <<'PY'
import json
import pathlib
import sys
import time

report_path = pathlib.Path(sys.argv[1])
history_path = pathlib.Path(sys.argv[2])
elapsed_ms = int(sys.argv[3])
iterations = int(sys.argv[4])
test_budget_ms = int(sys.argv[5])
suite_budget_ms = int(sys.argv[6])

report = {
    "kind": "genesis/task-concurrency-stress-v0.1",
    "timestamp_unix_s": int(time.time()),
    "iterations": iterations,
    "test_budget_ms": test_budget_ms,
    "suite_budget_ms": suite_budget_ms,
    "elapsed_ms": elapsed_ms,
    "ok": elapsed_ms <= suite_budget_ms,
    "matrix": [
        "cancellation-await",
        "channel-close-race",
        "parallel-reduce-bounded",
    ],
    "replay_equivalence_asserted": True,
}

report_path.parent.mkdir(parents=True, exist_ok=True)
report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")

history_path.parent.mkdir(parents=True, exist_ok=True)
with history_path.open("a", encoding="utf-8") as f:
    f.write(json.dumps(report, sort_keys=True) + "\n")

print(f"task-concurrency-stress: wrote report {report_path}")
print(
    f"task-concurrency-stress: elapsed_ms={elapsed_ms} iterations={iterations} "
    f"suite_budget_ms={suite_budget_ms}"
)

if elapsed_ms > suite_budget_ms:
    raise SystemExit(
        f"task-concurrency-stress: suite budget exceeded ({elapsed_ms}ms > {suite_budget_ms}ms)"
    )
PY

echo "task-concurrency-stress: ok"
