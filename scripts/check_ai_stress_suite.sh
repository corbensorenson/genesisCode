#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

TOTAL_BUDGET_MS="${GENESIS_STRESS_BUDGET_MS:-900000}"
REPORT_PATH="${GENESIS_STRESS_REPORT:-.genesis/perf/ai_stress_suite_metrics.json}"
HISTORY_PATH="${GENESIS_STRESS_HISTORY:-.genesis/perf/ai_stress_suite_history.jsonl}"

now_ns() {
  python3 - <<'PY'
import time
print(time.time_ns())
PY
}

mkdir -p "$(dirname "$REPORT_PATH")"
mkdir -p "$(dirname "$HISTORY_PATH")"

TMP_TSV="$(mktemp)"
cleanup() {
  rm -f "$TMP_TSV"
}
trap cleanup EXIT

run_case() {
  local label="$1"
  shift
  local start_ns end_ns elapsed_ms
  start_ns="$(now_ns)"
  "$@"
  end_ns="$(now_ns)"
  elapsed_ms="$(( (end_ns - start_ns) / 1000000 ))"
  printf "%s\t%s\n" "$label" "$elapsed_ms" >>"$TMP_TSV"
  echo "ai-stress-suite: ${label}=${elapsed_ms}ms"
}

echo "ai-stress-suite: running deterministic stress checks"
run_case bridge_gpu_compute_replay \
  cargo test -p gc_effects --test gfx_gpu_bridge --quiet
run_case editor_task_replay \
  cargo test -p gc_effects --test host_abi_surface editor_plugin_and_task_ops_are_replay_deterministic --quiet
run_case selfhost_parallel_obligations \
  cargo test -p gc_cli --test cli_selfhost_gpu_parallel --quiet

python3 - "$TMP_TSV" "$REPORT_PATH" "$HISTORY_PATH" "$TOTAL_BUDGET_MS" <<'PY'
import json
import os
import sys
import time

tsv_path, report_path, history_path, budget_ms_s = sys.argv[1:]
budget_ms = int(budget_ms_s)

checks = []
total_elapsed = 0
with open(tsv_path, "r", encoding="utf-8") as f:
    for line in f:
        line = line.strip()
        if not line:
            continue
        label, elapsed_s = line.split("\t", 1)
        elapsed = int(elapsed_s)
        checks.append({"name": label, "elapsed_ms": elapsed})
        total_elapsed += elapsed

required = {
    "bridge_gpu_compute_replay",
    "editor_task_replay",
    "selfhost_parallel_obligations",
}
have = {c["name"] for c in checks}
missing = sorted(required - have)
if missing:
    raise SystemExit(f"ai-stress-suite: missing required checks: {missing}")

report = {
    "kind": "genesis/ai-stress-suite-v0.1",
    "timestamp_unix_s": int(time.time()),
    "budget_total_ms": budget_ms,
    "total_elapsed_ms": total_elapsed,
    "checks": checks,
    "task_scheduling_verified": True,
    "bridge_budget_verified": True,
    "gpu_compute_verified": True,
    "replay_integrity_verified": True,
}

history = []
if os.path.exists(history_path):
    with open(history_path, "r", encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                row = json.loads(line)
            except Exception:
                continue
            if isinstance(row, dict) and row.get("kind") == "genesis/ai-stress-suite-v0.1":
                history.append(row)
history.append(report)
history = history[-200:]

with open(history_path, "w", encoding="utf-8") as f:
    for row in history:
        f.write(json.dumps(row, sort_keys=True))
        f.write("\n")

with open(report_path, "w", encoding="utf-8") as f:
    json.dump(report, f, indent=2, sort_keys=True)
    f.write("\n")

print(json.dumps(report, sort_keys=True))

if total_elapsed > budget_ms:
    raise SystemExit(
        f"ai-stress-suite: total_elapsed_ms {total_elapsed} exceeds budget {budget_ms}"
    )
PY

echo "ai-stress-suite: ok total<=${TOTAL_BUDGET_MS}ms report=$REPORT_PATH"
