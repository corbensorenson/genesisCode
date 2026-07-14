#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if [[ "$#" -ne 3 ]]; then
  echo "usage: $0 <report-output> <history-output> <history-input>" >&2
  exit 2
fi

REPORT_PATH="$1"
HISTORY_PATH="$2"
HISTORY_INPUT_PATH="$3"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
source "$ROOT_DIR/scripts/lib/profile_gate_timing.sh"
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "check-foundation-stdlib-conformance" \
  root-host

START_MS="$(genesis_profile_gate_now_ms)"
BUDGET_MS="${GENESIS_FOUNDATION_STDLIB_CONFORMANCE_BUDGET_MS:-300000}"

cargo test -p gc_prelude --test prelude_foundation_stdlib_conformance --quiet

BASELINE_HISTORY=""
if [[ "$HISTORY_INPUT_PATH" != "$HISTORY_PATH" && -f "$HISTORY_INPUT_PATH" ]]; then
  BASELINE_HISTORY="$HISTORY_INPUT_PATH"
fi

genesis_profile_gate_emit_runtime_report \
  "foundation-stdlib-conformance" \
  "genesis/foundation-stdlib-conformance-v0.1" \
  "$REPORT_PATH" \
  "$HISTORY_PATH" \
  "$START_MS" \
  "$BUDGET_MS" \
  "1" \
  "" \
  "" \
  "$BASELINE_HISTORY"

echo "foundation-stdlib-conformance: ok"
