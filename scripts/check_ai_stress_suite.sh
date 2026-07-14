#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

RETAINED_BASELINE_FILE="${GENESIS_CHECK_AI_STRESS_BASELINE_INPUT:-.genesis/perf/ai_stress_suite_history.jsonl}"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

bash scripts/render_ai_stress_suite_report.sh \
  "$TMP_DIR/ai_stress_suite_metrics.json" \
  "$TMP_DIR/ai_stress_suite_history.jsonl" \
  "$RETAINED_BASELINE_FILE"
