#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
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
  "check-fuzz-differential-hardening" \
  root-host

START_MS="$(genesis_profile_gate_now_ms)"
BUDGET_MS="${GENESIS_FUZZ_DIFFERENTIAL_HARDENING_BUDGET_MS:-900000}"

echo "fuzz-differential-hardening: parser/canonicalizer fuzz invariants"
cargo test -p gc_coreform --test fuzz_parse_print --quiet

echo "fuzz-differential-hardening: patch schema fuzz invariants"
cargo test -p gc_patches --test fuzz_patch --quiet

echo "fuzz-differential-hardening: effect log fuzz invariants"
cargo test -p gc_effects --test fuzz_log --quiet

echo "fuzz-differential-hardening: optimizer rewrite fuzz invariants"
cargo test -p gc_opt --test fuzz_optimizer --quiet

echo "fuzz-differential-hardening: malformed/adversarial differential corpus"
cargo test -p gc_cli --test cli_differential_adversarial --quiet

BASELINE_HISTORY=""
if [[ "$HISTORY_INPUT_PATH" != "$HISTORY_PATH" && -f "$HISTORY_INPUT_PATH" ]]; then
  BASELINE_HISTORY="$HISTORY_INPUT_PATH"
fi

genesis_profile_gate_emit_runtime_report \
  "fuzz-differential-hardening" \
  "genesis/fuzz-differential-hardening-v0.1" \
  "$REPORT_PATH" \
  "$HISTORY_PATH" \
  "$START_MS" \
  "$BUDGET_MS" \
  "1" \
  "" \
  "" \
  "$BASELINE_HISTORY"

echo "fuzz-differential-hardening: ok"
