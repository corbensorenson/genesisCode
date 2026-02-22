#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
source "$ROOT_DIR/scripts/lib/profile_gate_timing.sh"
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "check-foundation-stdlib-conformance" \
  ".genesis/build/cargo" \
  "GENESIS_CHECK_FOUNDATION_STDLIB_CONFORMANCE_CARGO_TARGET_DIR"

START_MS="$(genesis_profile_gate_now_ms)"
REPORT_PATH="${GENESIS_FOUNDATION_STDLIB_CONFORMANCE_REPORT:-.genesis/perf/foundation_stdlib_conformance_report.json}"
HISTORY_PATH="${GENESIS_FOUNDATION_STDLIB_CONFORMANCE_HISTORY:-.genesis/perf/foundation_stdlib_conformance_history.jsonl}"
BUDGET_MS="${GENESIS_FOUNDATION_STDLIB_CONFORMANCE_BUDGET_MS:-300000}"

cargo test -p gc_prelude --test prelude_foundation_stdlib_conformance --quiet

genesis_profile_gate_emit_runtime_report \
  "foundation-stdlib-conformance" \
  "genesis/foundation-stdlib-conformance-v0.1" \
  "$REPORT_PATH" \
  "$HISTORY_PATH" \
  "$START_MS" \
  "$BUDGET_MS"

echo "foundation-stdlib-conformance: ok"
