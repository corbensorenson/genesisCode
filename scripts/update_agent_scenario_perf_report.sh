#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

GAUNTLET_EVIDENCE_FILE="${GENESIS_AGENT_GAUNTLET_REPORT:-.genesis/perf/agent_capability_gauntlet_report.json}"
GAUNTLET_TIMING_FILE="${GENESIS_AGENT_GAUNTLET_HISTORY:-.genesis/perf/agent_capability_gauntlet_history.jsonl}"
PERSISTENT_REPORT_PATH="${GENESIS_AGENT_SCENARIO_REPORT:-.genesis/perf/agent_scenario_perf_report.json}"
PERSISTENT_HISTORY_PATH="${GENESIS_AGENT_SCENARIO_HISTORY:-.genesis/perf/agent_scenario_perf_history.jsonl}"
exec bash scripts/render_agent_scenario_perf_report.sh \
  "$PERSISTENT_REPORT_PATH" \
  "$PERSISTENT_HISTORY_PATH" \
  "$GAUNTLET_EVIDENCE_FILE" \
  "$GAUNTLET_TIMING_FILE"
