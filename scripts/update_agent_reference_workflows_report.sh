#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

PERSISTENT_REPORT_PATH="${GENESIS_AGENT_GAUNTLET_REPORT:-.genesis/perf/agent_capability_gauntlet_report.json}"
PERSISTENT_HISTORY_PATH="${GENESIS_AGENT_GAUNTLET_HISTORY:-.genesis/perf/agent_capability_gauntlet_history.jsonl}"
exec bash scripts/render_agent_reference_workflows_report.sh \
  "$PERSISTENT_REPORT_PATH" \
  "$PERSISTENT_HISTORY_PATH" \
  "$PERSISTENT_HISTORY_PATH"
