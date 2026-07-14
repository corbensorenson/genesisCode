#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if [[ "${CI:-}" == "true" ]]; then
  PROFILE="${GENESIS_HEALTH_PROFILE:-release-full}"
else
  PROFILE="${GENESIS_HEALTH_PROFILE:-dev-fast}"
fi
args=("$@")
for ((idx = 0; idx < ${#args[@]}; idx++)); do
  if [[ "${args[$idx]}" == "--profile" && $((idx + 1)) -lt ${#args[@]} ]]; then
    PROFILE="${args[$((idx + 1))]}"
  fi
done

if [[ "$PROFILE" == "agent-inner-loop" ]]; then
  DEFAULT_PROFILE_REPORT=".genesis/perf/upgrade_plan_health_agent_inner_loop_report.json"
else
  DEFAULT_PROFILE_REPORT=".genesis/perf/upgrade_plan_health_profile_report.json"
fi

exec bash scripts/render_upgrade_plan_health_report.sh \
  "${GENESIS_HEALTH_PROFILE_REPORT:-$DEFAULT_PROFILE_REPORT}" \
  "${GENESIS_HEALTH_PROFILE_HISTORY:-.genesis/perf/upgrade_plan_health_profile_history.jsonl}" \
  "${GENESIS_HEALTH_AGENT_INNER_LOOP_HISTORY:-.genesis/perf/upgrade_plan_health_agent_inner_loop_history.jsonl}" \
  "${GENESIS_HEALTH_PREPUSH_HISTORY:-.genesis/perf/upgrade_plan_health_prepush_history.jsonl}" \
  "${GENESIS_HEALTH_RELEASE_FULL_HISTORY:-.genesis/perf/upgrade_plan_health_release_full_history.jsonl}" \
  "${GENESIS_HEALTH_WARMUP_REPORT:-.genesis/perf/upgrade_plan_health_warmup_${PROFILE}.json}" \
  "${GENESIS_HEALTH_DISK_PREFLIGHT_REPORT:-.genesis/perf/upgrade_plan_health_disk_preflight_report.json}" \
  "$@"
