#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

BUDGET_MS="${GENESIS_HOST_BRIDGE_FAULT_BUDGET_MS:-45000}"
REPORT_PATH="${GENESIS_HOST_BRIDGE_FAULT_REPORT:-.genesis/perf/host_bridge_fault_injection_report.json}"
HISTORY_PATH="${GENESIS_HOST_BRIDGE_FAULT_HISTORY:-.genesis/perf/host_bridge_fault_injection_history.jsonl}"

if [[ ! "$BUDGET_MS" =~ ^[0-9]+$ || "$BUDGET_MS" -le 0 ]]; then
  echo "host-bridge-fault-injection: GENESIS_HOST_BRIDGE_FAULT_BUDGET_MS must be a positive integer" >&2
  exit 2
fi

start_ns="$(python3 - <<'PY'
import time
print(time.time_ns())
PY
)"

cargo test -p gc_effects --test host_bridge_fault_injection --quiet

end_ns="$(python3 - <<'PY'
import time
print(time.time_ns())
PY
)"
elapsed_ms="$(( (end_ns - start_ns) / 1000000 ))"

python3 - "$REPORT_PATH" "$HISTORY_PATH" "$elapsed_ms" "$BUDGET_MS" <<'PY'
import json
import pathlib
import sys
import time

report_path = pathlib.Path(sys.argv[1])
history_path = pathlib.Path(sys.argv[2])
elapsed_ms = int(sys.argv[3])
budget_ms = int(sys.argv[4])

report = {
    "kind": "genesis/host-bridge-fault-injection-v0.1",
    "timestamp_unix_s": int(time.time()),
    "elapsed_ms": elapsed_ms,
    "budget_ms": budget_ms,
    "ok": elapsed_ms <= budget_ms,
    "families": ["fs", "net", "process", "plugin"],
    "deterministic_replay_verified": True,
}

report_path.parent.mkdir(parents=True, exist_ok=True)
report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")

history_path.parent.mkdir(parents=True, exist_ok=True)
with history_path.open("a", encoding="utf-8") as f:
    f.write(json.dumps(report, sort_keys=True) + "\n")

print(f"host-bridge-fault-injection: wrote report {report_path}")
print(f"host-bridge-fault-injection: elapsed_ms={elapsed_ms} budget_ms={budget_ms}")

if elapsed_ms > budget_ms:
    raise SystemExit(
        f"host-bridge-fault-injection: budget exceeded ({elapsed_ms}ms > {budget_ms}ms)"
    )
PY

echo "host-bridge-fault-injection: ok"
