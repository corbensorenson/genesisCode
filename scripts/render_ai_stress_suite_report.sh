#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

if [[ "$#" -ne 3 ]]; then
  echo "usage: $0 <report-output> <history-output> <history-input>" >&2
  exit 2
fi

REPORT_PATH="$1"
HISTORY_PATH="$2"
HISTORY_INPUT="$3"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "check-ai-stress-suite" \
  root-host

TOTAL_BUDGET_MS="${GENESIS_STRESS_BUDGET_MS:-900000}"
FAULT_INJECT="${GENESIS_STRESS_FAULT_INJECT:-}"

now_ns() {
  python3 - <<'PY'
import time
print(time.time_ns())
PY
}

mkdir -p "$(dirname "$REPORT_PATH")"
mkdir -p "$(dirname "$HISTORY_PATH")"
if [[ "$HISTORY_INPUT" != "$HISTORY_PATH" ]]; then
  if [[ -f "$HISTORY_INPUT" ]]; then
    cp "$HISTORY_INPUT" "$HISTORY_PATH"
  else
    : >"$HISTORY_PATH"
  fi
fi

TMP_TSV="$(mktemp)"
cleanup() {
  rm -f "$TMP_TSV"
}
trap cleanup EXIT

run_case() {
  local label="$1"
  shift
  local start_ns end_ns elapsed_ms status
  status=0
  start_ns="$(now_ns)"
  if [[ ",$FAULT_INJECT," == *",$label,"* ]]; then
    status=86
  else
    if "$@"; then
      status=0
    else
      status=$?
    fi
  fi
  end_ns="$(now_ns)"
  elapsed_ms="$(( (end_ns - start_ns) / 1000000 ))"
  printf "%s\t%s\t%s\n" "$label" "$elapsed_ms" "$status" >>"$TMP_TSV"
  if [[ "$status" -eq 0 ]]; then
    echo "ai-stress-suite: ${label}=${elapsed_ms}ms status=ok"
  else
    echo "ai-stress-suite: ${label}=${elapsed_ms}ms status=fail($status)" >&2
  fi
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
status_by_name = {}
with open(tsv_path, "r", encoding="utf-8") as f:
    for line in f:
        line = line.strip()
        if not line:
            continue
        parts = line.split("\t")
        if len(parts) != 3:
            raise SystemExit(f"ai-stress-suite: malformed record: {line}")
        label, elapsed_s, status_s = parts
        elapsed = int(elapsed_s)
        status = int(status_s)
        passed = status == 0
        checks.append(
            {"name": label, "elapsed_ms": elapsed, "status": status, "passed": passed}
        )
        status_by_name[label] = passed
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

failed = sorted(name for name in required if not status_by_name.get(name, False))
bridge_verified = status_by_name.get("bridge_gpu_compute_replay", False)
task_verified = (
    status_by_name.get("editor_task_replay", False)
    and status_by_name.get("selfhost_parallel_obligations", False)
)
replay_verified = len(failed) == 0

report = {
    "kind": "genesis/ai-stress-suite-v0.1",
    "timestamp_unix_s": int(time.time()),
    "budget_total_ms": budget_ms,
    "total_elapsed_ms": total_elapsed,
    "checks": checks,
    "failed_checks": failed,
    "task_scheduling_verified": task_verified,
    "bridge_budget_verified": bridge_verified,
    "gpu_compute_verified": bridge_verified,
    "replay_integrity_verified": replay_verified,
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

if failed:
    raise SystemExit(
        "ai-stress-suite: one or more required checks failed: "
        + ", ".join(failed)
    )

if total_elapsed > budget_ms:
    raise SystemExit(
        f"ai-stress-suite: total_elapsed_ms {total_elapsed} exceeds budget {budget_ms}"
    )
PY

echo "ai-stress-suite: ok total<=${TOTAL_BUDGET_MS}ms report=$REPORT_PATH"
