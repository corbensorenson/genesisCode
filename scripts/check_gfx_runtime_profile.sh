#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
source "$ROOT_DIR/scripts/lib/profile_gate_timing.sh"

genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "check-gfx-runtime-profile" \
  ".genesis/build/cargo" \
  "GENESIS_CHECK_GFX_RUNTIME_PROFILE_CARGO_TARGET_DIR"

START_MS="$(genesis_profile_gate_now_ms)"

WORKFLOW_SCRIPT="${GENESIS_GFX_RUNTIME_PROFILE_WORKFLOW:-$ROOT_DIR/examples/agent_long_running_gfx_loop_workflow/workflow.sh}"
REPORT_PATH="${GENESIS_GFX_RUNTIME_PROFILE_OUT:-.genesis/perf/gfx_runtime_profile_report.json}"
RUNTIME_REPORT="${GENESIS_GFX_RUNTIME_PROFILE_RUNTIME_REPORT_OUT:-.genesis/perf/gfx_runtime_profile_runtime_report.json}"
RUNTIME_HISTORY="${GENESIS_GFX_RUNTIME_PROFILE_RUNTIME_HISTORY_OUT:-.genesis/perf/gfx_runtime_profile_runtime_history.jsonl}"
RUNTIME_BUDGET_MS="${GENESIS_GFX_RUNTIME_PROFILE_RUNTIME_BUDGET_MS:-900000}"
SKIP_RUN="${GENESIS_GFX_RUNTIME_PROFILE_SKIP_RUN:-0}"

if [[ "$SKIP_RUN" == "1" ]]; then
  [[ -f "$REPORT_PATH" ]] || {
    echo "gfx-runtime-profile: GENESIS_GFX_RUNTIME_PROFILE_SKIP_RUN=1 requires existing report: $REPORT_PATH" >&2
    exit 2
  }
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

genesis_profile_gate_emit_runtime_report \
  "gfx-runtime-profile" \
  "genesis/gfx-runtime-profile-runtime-v0.1" \
  "$RUNTIME_REPORT" \
  "$RUNTIME_HISTORY" \
  "$START_MS" \
  "$RUNTIME_BUDGET_MS" \
  "1" \
  "{\"profile_report\":\"$REPORT_PATH\"}"

echo "gfx-runtime-profile: ok"
