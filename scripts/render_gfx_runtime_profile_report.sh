#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

if [[ "$#" -ne 5 ]]; then
  echo "usage: $0 <profile-report-output> <runtime-report-output> <runtime-history-output> <profile-report-input> <runtime-history-input>" >&2
  exit 2
fi

REPORT_PATH="$1"
RUNTIME_REPORT="$2"
RUNTIME_HISTORY="$3"
PROFILE_INPUT_PATH="$4"
RUNTIME_HISTORY_INPUT="$5"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
source "$ROOT_DIR/scripts/lib/profile_gate_timing.sh"

genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "check-gfx-runtime-profile" \
  root-host

START_MS="$(genesis_profile_gate_now_ms)"

WORKFLOW_SCRIPT="${GENESIS_GFX_RUNTIME_PROFILE_WORKFLOW:-$ROOT_DIR/examples/agent_long_running_gfx_loop_workflow/workflow.sh}"
RUNTIME_BASELINE_HISTORY="${GENESIS_GFX_RUNTIME_PROFILE_RUNTIME_BASELINE_HISTORY_OUT:-policies/perf/gfx_runtime_profile_runtime_seed_history.jsonl}"
RUNTIME_BUDGET_MS="${GENESIS_GFX_RUNTIME_PROFILE_RUNTIME_BUDGET_MS:-900000}"
RUNTIME_MIN_HISTORY="${GENESIS_GFX_RUNTIME_PROFILE_RUNTIME_MIN_HISTORY:-5}"
RUNTIME_REQUIRE_MIN_HISTORY="${GENESIS_GFX_RUNTIME_PROFILE_RUNTIME_REQUIRE_MIN_HISTORY:-1}"
SKIP_RUN="${GENESIS_GFX_RUNTIME_PROFILE_SKIP_RUN:-0}"

if [[ ! "$RUNTIME_MIN_HISTORY" =~ ^[0-9]+$ || "$RUNTIME_MIN_HISTORY" -le 0 ]]; then
  echo "gfx-runtime-profile: GENESIS_GFX_RUNTIME_PROFILE_RUNTIME_MIN_HISTORY must be a positive integer" >&2
  exit 2
fi
if [[ "$RUNTIME_REQUIRE_MIN_HISTORY" != "0" && "$RUNTIME_REQUIRE_MIN_HISTORY" != "1" ]]; then
  echo "gfx-runtime-profile: GENESIS_GFX_RUNTIME_PROFILE_RUNTIME_REQUIRE_MIN_HISTORY must be 0 or 1" >&2
  exit 2
fi

if [[ "$SKIP_RUN" == "1" ]]; then
  [[ -f "$PROFILE_INPUT_PATH" ]] || {
    echo "gfx-runtime-profile: GENESIS_GFX_RUNTIME_PROFILE_SKIP_RUN=1 requires existing report: $PROFILE_INPUT_PATH" >&2
    exit 2
  }
  if [[ "$PROFILE_INPUT_PATH" != "$REPORT_PATH" ]]; then
    mkdir -p "$(dirname "$REPORT_PATH")"
    cp "$PROFILE_INPUT_PATH" "$REPORT_PATH"
  fi
  echo "gfx-runtime-profile: skipping workflow run (GENESIS_GFX_RUNTIME_PROFILE_SKIP_RUN=1)"
else
  [[ -f "$WORKFLOW_SCRIPT" ]] || {
    echo "gfx-runtime-profile: missing workflow script: $WORKFLOW_SCRIPT" >&2
    exit 1
  }
  echo "gfx-runtime-profile: running gfx workflow lane"
  if [[ -x "$WORKFLOW_SCRIPT" ]]; then
    WORKFLOW_OUT="$("$WORKFLOW_SCRIPT")"
  else
    WORKFLOW_OUT="$(bash "$WORKFLOW_SCRIPT")"
  fi
  python3 - "$WORKFLOW_OUT" "$WORKFLOW_SCRIPT" "$REPORT_PATH" <<'PY'
import datetime as dt
import hashlib
import json
import pathlib
import re
import sys

workflow_out = sys.argv[1].strip()
workflow_script = sys.argv[2]
report_path = pathlib.Path(sys.argv[3])

if "agent-long-running-gfx-loop-workflow: ok" not in workflow_out:
    raise SystemExit(
        "gfx-runtime-profile: workflow output missing success marker for agent_long_running_gfx_loop_workflow"
    )

match = re.search(
    r"acceptance=([0-9a-f]{64})\s+vcs=([0-9a-f]{64})\s+replay=(.+)$",
    workflow_out,
)
if not match:
    raise SystemExit(
        "gfx-runtime-profile: workflow output missing acceptance/vcs/replay payload"
    )

acceptance_h = match.group(1)
vcs_h = match.group(2)
replay = match.group(3)

if ":frames 64" not in replay:
    raise SystemExit(
        "gfx-runtime-profile: replay payload missing expected fixed-loop frame count marker"
    )

report = {
    "kind": "genesis/gfx-runtime-profile-v0.1",
    "ok": True,
    "workflow": "agent_long_running_gfx_loop_workflow",
    "workflow_script": workflow_script,
    "acceptance_h": acceptance_h,
    "vcs_h": vcs_h,
    "replay": replay,
    "replay_h": hashlib.sha256(replay.encode("utf-8")).hexdigest(),
    "stack": "gfx-only",
    "cross_over_shared_primitives": ["gfx/gpu::*", "gpu/compute::*"],
    "timestamp_utc": dt.datetime.now(dt.timezone.utc).replace(microsecond=0).isoformat(),
}

report_path.parent.mkdir(parents=True, exist_ok=True)
report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"gfx-runtime-profile: wrote report {report_path}")
PY
fi

EFFECTIVE_BASELINE_HISTORY="$RUNTIME_BASELINE_HISTORY"
MERGED_BASELINE_HISTORY=""
if [[ "$RUNTIME_HISTORY_INPUT" != "$RUNTIME_HISTORY" && -f "$RUNTIME_HISTORY_INPUT" ]]; then
  MERGED_BASELINE_HISTORY="$(mktemp)"
  cat "$RUNTIME_BASELINE_HISTORY" "$RUNTIME_HISTORY_INPUT" >"$MERGED_BASELINE_HISTORY"
  EFFECTIVE_BASELINE_HISTORY="$MERGED_BASELINE_HISTORY"
fi
cleanup_baseline() {
  if [[ -n "$MERGED_BASELINE_HISTORY" ]]; then
    rm -f "$MERGED_BASELINE_HISTORY"
  fi
}
trap cleanup_baseline EXIT

genesis_profile_gate_emit_runtime_report \
  "gfx-runtime-profile" \
  "genesis/gfx-runtime-profile-runtime-v0.1" \
  "$RUNTIME_REPORT" \
  "$RUNTIME_HISTORY" \
  "$START_MS" \
  "$RUNTIME_BUDGET_MS" \
  "$RUNTIME_MIN_HISTORY" \
  "{\"profile_report\":\"$REPORT_PATH\"}" \
  "" \
  "$EFFECTIVE_BASELINE_HISTORY" \
  "$RUNTIME_REQUIRE_MIN_HISTORY"

echo "gfx-runtime-profile: ok"
