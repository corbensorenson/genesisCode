#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

REFRESH_CRITICAL_INPUTS="${GENESIS_SELFHOST_READINESS_REFRESH_CRITICAL_REPORTS:-0}"
if [[ "$REFRESH_CRITICAL_INPUTS" != "0" && "$REFRESH_CRITICAL_INPUTS" != "1" ]]; then
  echo "selfhost-readiness: GENESIS_SELFHOST_READINESS_REFRESH_CRITICAL_REPORTS must be 0 or 1" >&2
  exit 2
fi
if [[ "$REFRESH_CRITICAL_INPUTS" == "1" ]]; then
  echo "selfhost-readiness: checks are read-only; run the explicit producers reported for missing or stale inputs" >&2
  exit 2
fi

PREVIOUS_INPUT_FILE="${GENESIS_SELFHOST_READINESS_REPORT:-.genesis/perf/selfhost_readiness_report.json}"
BASELINE_INPUT_FILE="${GENESIS_SELFHOST_READINESS_HISTORY:-.genesis/perf/selfhost_readiness_history.jsonl}"
TMP_DIR="$(mktemp -d)"
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

bash scripts/render_selfhost_readiness_scorecard_report.sh \
  "$TMP_DIR/selfhost_readiness_report.json" \
  "$TMP_DIR/selfhost_readiness_history.jsonl" \
  "$PREVIOUS_INPUT_FILE" \
  "$BASELINE_INPUT_FILE"
