#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

TMP_DIR=""
REMOVE_TMP_DIR=1
if [[ -n "${GENESIS_CHECK_HEALTH_OUTPUT_ROOT:-}" ]]; then
  TMP_DIR="$(python3 - "${GENESIS_CHECK_HEALTH_OUTPUT_ROOT}" "${GENESIS_CHECK_HEALTH_OUTPUT_CONTAINMENT_ROOT:-}" "$ROOT_DIR" <<'PY'
import pathlib
import sys

raw_output, raw_containment, raw_repo = sys.argv[1:]
if not raw_containment:
    raise SystemExit(
        "upgrade-plan-health: retained private output requires "
        "GENESIS_CHECK_HEALTH_OUTPUT_CONTAINMENT_ROOT"
    )
output = pathlib.Path(raw_output)
containment = pathlib.Path(raw_containment)
repo = pathlib.Path(raw_repo).resolve(strict=True)
if not output.is_absolute() or not containment.is_absolute():
    raise SystemExit("upgrade-plan-health: private output paths must be absolute")
containment = containment.resolve(strict=True)
output = output.resolve(strict=True)
if output.parent != containment:
    raise SystemExit("upgrade-plan-health: private output must be a direct containment child")
if containment == repo or repo in containment.parents or containment in repo.parents:
    raise SystemExit("upgrade-plan-health: private output containment must be outside the repository")
if any(output.iterdir()):
    raise SystemExit("upgrade-plan-health: private output directory must start empty")
print(output)
PY
)"
  REMOVE_TMP_DIR=0
else
  TMP_DIR="$(mktemp -d)"
fi

cleanup_private_output() {
  if [[ "$REMOVE_TMP_DIR" == "1" ]]; then
    rm -rf "$TMP_DIR"
  fi
}
trap cleanup_private_output EXIT

copy_history_input() {
  local source="$1"
  local destination="$2"
  if [[ -f "$source" ]]; then
    cp "$source" "$destination"
  fi
}

copy_history_input \
  "${GENESIS_CHECK_HEALTH_PROFILE_HISTORY_INPUT:-.genesis/perf/upgrade_plan_health_profile_history.jsonl}" \
  "$TMP_DIR/profile-history.jsonl"
copy_history_input \
  "${GENESIS_CHECK_HEALTH_AGENT_INNER_LOOP_HISTORY_INPUT:-.genesis/perf/upgrade_plan_health_agent_inner_loop_history.jsonl}" \
  "$TMP_DIR/agent-inner-loop-history.jsonl"
copy_history_input \
  "${GENESIS_CHECK_HEALTH_PREPUSH_HISTORY_INPUT:-.genesis/perf/upgrade_plan_health_prepush_history.jsonl}" \
  "$TMP_DIR/prepush-history.jsonl"
copy_history_input \
  "${GENESIS_CHECK_HEALTH_RELEASE_FULL_HISTORY_INPUT:-.genesis/perf/upgrade_plan_health_release_full_history.jsonl}" \
  "$TMP_DIR/release-full-history.jsonl"

exec env \
  GENESIS_HEALTH_PROFILE_GATE_CACHE=0 \
  GENESIS_HEALTH_WARM_CARGO_CACHE=0 \
  bash scripts/render_upgrade_plan_health_report.sh \
  "$TMP_DIR/profile-report.json" \
  "$TMP_DIR/profile-history.jsonl" \
  "$TMP_DIR/agent-inner-loop-history.jsonl" \
  "$TMP_DIR/prepush-history.jsonl" \
  "$TMP_DIR/release-full-history.jsonl" \
  "$TMP_DIR/warmup-report.json" \
  "$TMP_DIR/disk-preflight-report.json" \
  "$@"
