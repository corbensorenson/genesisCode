#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

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
