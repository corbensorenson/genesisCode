#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

BASELINE_FILE="${GENESIS_TASK_STRESS_HISTORY:-.genesis/perf/task_concurrency_stress_history.jsonl}"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

bash scripts/render_task_concurrency_stress_report.sh \
  "$TMP_DIR/task_concurrency_stress_report.json" \
  "$TMP_DIR/task_concurrency_stress_history.jsonl" \
  "$BASELINE_FILE"
