#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
source "$ROOT_DIR/scripts/lib/profile_gate_timing.sh"
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "check-fuzz-differential-hardening" \
  ".genesis/build/cargo" \
  "GENESIS_CHECK_FUZZ_DIFFERENTIAL_HARDENING_CARGO_TARGET_DIR"

START_MS="$(genesis_profile_gate_now_ms)"
REPORT_PATH="${GENESIS_FUZZ_DIFFERENTIAL_HARDENING_REPORT:-.genesis/perf/fuzz_differential_hardening_report.json}"
HISTORY_PATH="${GENESIS_FUZZ_DIFFERENTIAL_HARDENING_HISTORY:-.genesis/perf/fuzz_differential_hardening_history.jsonl}"
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

genesis_profile_gate_emit_runtime_report \
  "fuzz-differential-hardening" \
  "genesis/fuzz-differential-hardening-v0.1" \
  "$REPORT_PATH" \
  "$HISTORY_PATH" \
  "$START_MS" \
  "$BUDGET_MS"

echo "fuzz-differential-hardening: ok"
