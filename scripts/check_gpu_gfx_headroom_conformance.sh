#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "gpu-gfx-headroom-conformance" \
  ".genesis/build/cargo" \
  "GENESIS_GPU_GFX_HEADROOM_CARGO_TARGET_DIR"

source "$ROOT_DIR/scripts/lib/heavy_gate_preflight.sh"

REPORT_OUT="${GENESIS_GPU_GFX_HEADROOM_REPORT_OUT:-.genesis/perf/gpu_gfx_headroom_conformance_report.json}"
HISTORY_OUT="${GENESIS_GPU_GFX_HEADROOM_HISTORY_OUT:-.genesis/perf/gpu_gfx_headroom_conformance_history.jsonl}"
BUDGET_MS="${GENESIS_GPU_GFX_HEADROOM_BUDGET_MS:-600000}"
TMP_ROOT_BASE="${GENESIS_GPU_GFX_HEADROOM_TMPDIR:-$ROOT_DIR/.genesis/tmp/check-gpu-gfx-headroom-conformance}"
NORMAL_MIN_FREE_KB="${GENESIS_GPU_GFX_HEADROOM_NORMAL_MIN_FREE_KB:-3145728}"
NORMAL_AUTO_RECLAIM="${GENESIS_GPU_GFX_HEADROOM_NORMAL_AUTO_RECLAIM:-1}"
NORMAL_STRICT_MODE="${GENESIS_GPU_GFX_HEADROOM_NORMAL_STRICT_MODE:-auto}"
LOW_AUTO_RECLAIM="${GENESIS_GPU_GFX_HEADROOM_LOW_AUTO_RECLAIM:-0}"
LOW_STRICT_MODE="${GENESIS_GPU_GFX_HEADROOM_LOW_STRICT_MODE:-0}"
LOW_MIN_FREE_KB="${GENESIS_GPU_GFX_HEADROOM_LOW_MIN_FREE_KB:-}"
GENESIS_BIN="${GENESIS_BIN:-$CARGO_TARGET_DIR/debug/genesis}"

if [[ ! "$BUDGET_MS" =~ ^[0-9]+$ || "$BUDGET_MS" -le 0 ]]; then
  echo "gpu-gfx-headroom: GENESIS_GPU_GFX_HEADROOM_BUDGET_MS must be a positive integer" >&2
  exit 2
fi
if [[ ! "$NORMAL_MIN_FREE_KB" =~ ^[0-9]+$ || "$NORMAL_MIN_FREE_KB" -le 0 ]]; then
  echo "gpu-gfx-headroom: GENESIS_GPU_GFX_HEADROOM_NORMAL_MIN_FREE_KB must be a positive integer" >&2
  exit 2
fi
if [[ "$NORMAL_AUTO_RECLAIM" != "0" && "$NORMAL_AUTO_RECLAIM" != "1" ]]; then
  echo "gpu-gfx-headroom: GENESIS_GPU_GFX_HEADROOM_NORMAL_AUTO_RECLAIM must be 0 or 1" >&2
  exit 2
fi
if [[ "$NORMAL_STRICT_MODE" != "auto" && "$NORMAL_STRICT_MODE" != "0" && "$NORMAL_STRICT_MODE" != "1" ]]; then
  echo "gpu-gfx-headroom: GENESIS_GPU_GFX_HEADROOM_NORMAL_STRICT_MODE must be auto, 0, or 1" >&2
  exit 2
fi
if [[ "$LOW_AUTO_RECLAIM" != "0" && "$LOW_AUTO_RECLAIM" != "1" ]]; then
  echo "gpu-gfx-headroom: GENESIS_GPU_GFX_HEADROOM_LOW_AUTO_RECLAIM must be 0 or 1" >&2
  exit 2
fi
if [[ "$LOW_STRICT_MODE" != "0" && "$LOW_STRICT_MODE" != "1" ]]; then
  echo "gpu-gfx-headroom: GENESIS_GPU_GFX_HEADROOM_LOW_STRICT_MODE must be 0 or 1" >&2
  exit 2
fi

if [[ ! -x "$GENESIS_BIN" ]]; then
  cargo build -p gc_cli >/dev/null
fi

read_free_kb() {
  df -Pk "$ROOT_DIR" | awk 'NR==2 {print $4}'
}

FREE_KB_START="$(read_free_kb)"
if [[ -z "$LOW_MIN_FREE_KB" ]]; then
  LOW_MIN_FREE_KB=$((FREE_KB_START + 1048576))
fi
if [[ ! "$LOW_MIN_FREE_KB" =~ ^[0-9]+$ || "$LOW_MIN_FREE_KB" -le 0 ]]; then
  echo "gpu-gfx-headroom: GENESIS_GPU_GFX_HEADROOM_LOW_MIN_FREE_KB must be a positive integer" >&2
  exit 2
fi
LOW_HEADROOM_SIMULATED=0
if (( FREE_KB_START < LOW_MIN_FREE_KB )); then
  LOW_HEADROOM_SIMULATED=1
fi

START_MS="$(python3 - <<'PY'
import time
print(int(time.time() * 1000))
PY
)"

TMP_DIR="$(mktemp -d)"
RESULTS_DIR="$TMP_DIR/results"
mkdir -p "$RESULTS_DIR"
trap 'rm -rf "$TMP_DIR"' EXIT

FAILURES=()

run_lane_workflows() {
  local lane="$1"
  local strict_mode="$2"
  local min_free_kb="$3"
  local auto_reclaim="$4"
  local lane_tmp="$TMP_ROOT_BASE/$lane"
  local lane_results="$RESULTS_DIR/$lane"
  mkdir -p "$lane_results"

  genesis_heavy_gate_preflight \
    "$ROOT_DIR" \
    "gpu-gfx-headroom-$lane" \
    "$min_free_kb" \
    "$lane_tmp" \
    "$auto_reclaim" \
    "$strict_mode"

  local workflows=(
    "agent_gpu_compute_workflow:$ROOT_DIR/examples/agent_gpu_compute_workflow/workflow.sh"
    "agent_interactive_gfx_compute_workflow:$ROOT_DIR/examples/agent_interactive_gfx_compute_workflow/workflow.sh"
  )
  local entry
  for entry in "${workflows[@]}"; do
    local wf_name="${entry%%:*}"
    local wf_path="${entry#*:}"
    local wf_dir="$lane_results/$wf_name"
    mkdir -p "$wf_dir"

    local run1_ok=1
    local run2_ok=1
    local replay1=""
    local replay2=""

    if ! TMPDIR="$lane_tmp" GENESIS_BIN="$GENESIS_BIN" bash "$wf_path" >"$wf_dir/run1.stdout" 2>"$wf_dir/run1.stderr"; then
      run1_ok=0
      FAILURES+=("$lane/$wf_name:run1-failed")
    fi
    if [[ "$run1_ok" == "1" ]]; then
      replay1="$(
        {
          cat "$wf_dir/run1.stdout"
          cat "$wf_dir/run1.stderr"
        } | sed -n 's/.*replay=//p' | tail -n 1 | tr -d '\n'
      )"
      if [[ -z "$replay1" ]]; then
        FAILURES+=("$lane/$wf_name:run1-missing-replay")
      fi
    fi

    if ! TMPDIR="$lane_tmp" GENESIS_BIN="$GENESIS_BIN" bash "$wf_path" >"$wf_dir/run2.stdout" 2>"$wf_dir/run2.stderr"; then
      run2_ok=0
      FAILURES+=("$lane/$wf_name:run2-failed")
    fi
    if [[ "$run2_ok" == "1" ]]; then
      replay2="$(
        {
          cat "$wf_dir/run2.stdout"
          cat "$wf_dir/run2.stderr"
        } | sed -n 's/.*replay=//p' | tail -n 1 | tr -d '\n'
      )"
      if [[ -z "$replay2" ]]; then
        FAILURES+=("$lane/$wf_name:run2-missing-replay")
      fi
    fi

    if [[ -n "$replay1" && -n "$replay2" && "$replay1" != "$replay2" ]]; then
      FAILURES+=("$lane/$wf_name:replay-mismatch")
    fi

    printf '%s\n' "$replay1" >"$wf_dir/replay1.txt"
    printf '%s\n' "$replay2" >"$wf_dir/replay2.txt"
  done
}

run_lane_workflows "normal" "$NORMAL_STRICT_MODE" "$NORMAL_MIN_FREE_KB" "$NORMAL_AUTO_RECLAIM"
run_lane_workflows "low-headroom" "$LOW_STRICT_MODE" "$LOW_MIN_FREE_KB" "$LOW_AUTO_RECLAIM"

END_MS="$(python3 - <<'PY'
import time
print(int(time.time() * 1000))
PY
)"
ELAPSED_MS=$((END_MS - START_MS))

failure_args=()
if ((${#FAILURES[@]} > 0)); then
  failure_args=("${FAILURES[@]}")
fi

python3 - "$RESULTS_DIR" "$REPORT_OUT" "$HISTORY_OUT" "$ELAPSED_MS" "$BUDGET_MS" "$FREE_KB_START" "$LOW_MIN_FREE_KB" "$LOW_HEADROOM_SIMULATED" "${failure_args[@]-}" <<'PY'
import datetime as dt
import json
import pathlib
import sys

results_dir = pathlib.Path(sys.argv[1])
report_out = pathlib.Path(sys.argv[2])
history_out = pathlib.Path(sys.argv[3])
elapsed_ms = int(sys.argv[4])
budget_ms = int(sys.argv[5])
free_kb_start = int(sys.argv[6])
low_min_free_kb = int(sys.argv[7])
low_headroom_simulated = sys.argv[8] == "1"
failures = [x for x in sys.argv[9:] if x]

lanes = {}
for lane_dir in sorted(p for p in results_dir.iterdir() if p.is_dir()):
    lane_name = lane_dir.name
    workflows = []
    for wf_dir in sorted(p for p in lane_dir.iterdir() if p.is_dir()):
        replay1 = (wf_dir / "replay1.txt").read_text(encoding="utf-8").strip()
        replay2 = (wf_dir / "replay2.txt").read_text(encoding="utf-8").strip()
        run1_stdout = (wf_dir / "run1.stdout").read_text(encoding="utf-8")
        run2_stdout = (wf_dir / "run2.stdout").read_text(encoding="utf-8")
        run1_stderr = (wf_dir / "run1.stderr").read_text(encoding="utf-8")
        run2_stderr = (wf_dir / "run2.stderr").read_text(encoding="utf-8")
        wf_failures = [f for f in failures if f.startswith(f"{lane_name}/{wf_dir.name}:")]
        workflows.append(
            {
                "workflow": wf_dir.name,
                "ok": len(wf_failures) == 0,
                "failures": wf_failures,
                "replay1": replay1,
                "replay2": replay2,
                "run1_stdout_tail": run1_stdout[-300:],
                "run2_stdout_tail": run2_stdout[-300:],
                "run1_stderr_tail": run1_stderr[-300:],
                "run2_stderr_tail": run2_stderr[-300:],
            }
        )
    lanes[lane_name] = {
        "workflow_count": len(workflows),
        "workflow_successes": sum(1 for wf in workflows if wf["ok"]),
        "ok": all(wf["ok"] for wf in workflows),
        "workflows": workflows,
    }

fail_reasons = []
if failures:
    fail_reasons.append("workflow-failures")
if elapsed_ms > budget_ms:
    fail_reasons.append("elapsed-budget")
if not low_headroom_simulated:
    fail_reasons.append("low-headroom-not-simulated")

ok = not fail_reasons
now_utc = dt.datetime.now(dt.timezone.utc).replace(microsecond=0).isoformat()
doc = {
    "kind": "genesis/gpu-gfx-headroom-conformance-v0.1",
    "ok": ok,
    "elapsed_ms": elapsed_ms,
    "budget_ms": budget_ms,
    "free_kb_start": free_kb_start,
    "low_headroom_min_free_kb": low_min_free_kb,
    "low_headroom_simulated": low_headroom_simulated,
    "lanes": lanes,
    "failures": failures,
    "fail_reasons": fail_reasons,
    "timestamp_utc": now_utc,
}

if report_out.is_file():
    try:
        prev = json.loads(report_out.read_text(encoding="utf-8"))
        if isinstance(prev, dict) and isinstance(prev.get("elapsed_ms"), int):
            doc["previous_elapsed_ms"] = int(prev["elapsed_ms"])
            doc["elapsed_delta_ms"] = elapsed_ms - int(prev["elapsed_ms"])
    except json.JSONDecodeError:
        pass

report_out.parent.mkdir(parents=True, exist_ok=True)
history_out.parent.mkdir(parents=True, exist_ok=True)
report_out.write_text(json.dumps(doc, indent=2, sort_keys=True) + "\n", encoding="utf-8")
with history_out.open("a", encoding="utf-8") as fh:
    fh.write(
        json.dumps(
            {
                "kind": doc["kind"],
                "ok": ok,
                "elapsed_ms": elapsed_ms,
                "budget_ms": budget_ms,
                "low_headroom_simulated": low_headroom_simulated,
                "timestamp_utc": now_utc,
            },
            sort_keys=True,
        )
        + "\n"
    )

print(
    "gpu-gfx-headroom: "
    f"report={report_out} ok={ok} elapsed_ms={elapsed_ms} budget_ms={budget_ms} "
    f"low_headroom_simulated={low_headroom_simulated}"
)
if fail_reasons:
    raise SystemExit(
        "gpu-gfx-headroom: fail reasons: " + ", ".join(fail_reasons)
    )
PY

echo "gpu-gfx-headroom: ok"
