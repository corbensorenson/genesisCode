#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

REFRESH_CRITICAL_INPUTS="${GENESIS_SELFHOST_READINESS_REFRESH_CRITICAL_REPORTS:-0}"
if [[ "$REFRESH_CRITICAL_INPUTS" != "0" && "$REFRESH_CRITICAL_INPUTS" != "1" ]]; then
  echo "update-selfhost-readiness: GENESIS_SELFHOST_READINESS_REFRESH_CRITICAL_REPORTS must be 0 or 1" >&2
  exit 2
fi
if [[ "$REFRESH_CRITICAL_INPUTS" == "1" ]]; then
  echo "update-selfhost-readiness: this producer does not refresh prerequisite evidence; run each reported producer explicitly" >&2
  exit 2
fi

PERSISTENT_REPORT_PATH="${GENESIS_SELFHOST_READINESS_REPORT:-.genesis/perf/selfhost_readiness_report.json}"
PERSISTENT_HISTORY_PATH="${GENESIS_SELFHOST_READINESS_HISTORY:-.genesis/perf/selfhost_readiness_history.jsonl}"

exec bash scripts/render_selfhost_readiness_scorecard_report.sh \
  "$PERSISTENT_REPORT_PATH" \
  "$PERSISTENT_HISTORY_PATH" \
  "$PERSISTENT_REPORT_PATH" \
  "$PERSISTENT_HISTORY_PATH"
