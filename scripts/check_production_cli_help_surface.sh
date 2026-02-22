#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

source "$ROOT/scripts/lib/cargo_target_dir.sh"
source "$ROOT/scripts/lib/profile_gate_timing.sh"
source "$ROOT/scripts/lib/release_bin.sh"
genesis_configure_cargo_target_dir \
  "$ROOT" \
  "check-production-cli-help-surface" \
  ".genesis/build/cargo" \
  "GENESIS_CHECK_PRODUCTION_CLI_HELP_SURFACE_CARGO_TARGET_DIR"

START_MS="$(genesis_profile_gate_now_ms)"
REPORT_PATH="${GENESIS_PRODUCTION_CLI_HELP_SURFACE_REPORT:-.genesis/perf/production_cli_help_surface_report.json}"
HISTORY_PATH="${GENESIS_PRODUCTION_CLI_HELP_SURFACE_HISTORY:-.genesis/perf/production_cli_help_surface_history.jsonl}"
BUDGET_MS="${GENESIS_PRODUCTION_CLI_HELP_SURFACE_BUDGET_MS:-300000}"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

run_help() {
  local bin_path="$1"
  local out="$2"
  shift 2
  "$bin_path" "$@" --help >"$out"
}

assert_not_contains() {
  local file="$1"
  local needle="$2"
  if grep -Fq "$needle" "$file"; then
    echo "help-surface: unexpected token in $file: $needle"
    exit 1
  fi
}

assert_contains() {
  local file="$1"
  local needle="$2"
  if ! grep -Fq "$needle" "$file"; then
    echo "help-surface: expected token missing in $file: $needle"
    exit 1
  fi
}

GENESIS_HELP="$TMP/genesis.help"
GENESIS_WASI_HELP="$TMP/genesis_wasi.help"
GENESIS_PARITY_HELP="$TMP/genesis_parity.help"
GENESIS_WASI_PARITY_HELP="$TMP/genesis_wasi_parity.help"
GENESIS_FMT_HELP="$TMP/genesis_fmt.help"
GENESIS_WASI_FMT_HELP="$TMP/genesis_wasi_fmt.help"
GENESIS_PARITY_FMT_HELP="$TMP/genesis_parity_fmt.help"
GENESIS_WASI_PARITY_FMT_HELP="$TMP/genesis_wasi_parity_fmt.help"

GENESIS_BIN="$(genesis_build_release_bin gc_cli genesis)"
GENESIS_WASI_BIN="$(genesis_build_release_bin gc_wasi_cli genesis_wasi)"
GENESIS_PARITY_BIN="$(genesis_build_release_bin gc_cli genesis_parity)"
GENESIS_WASI_PARITY_BIN="$(genesis_build_release_bin gc_wasi_cli genesis_wasi_parity)"

run_help "$GENESIS_BIN" "$GENESIS_HELP"
run_help "$GENESIS_WASI_BIN" "$GENESIS_WASI_HELP"
run_help "$GENESIS_PARITY_BIN" "$GENESIS_PARITY_HELP"
run_help "$GENESIS_WASI_PARITY_BIN" "$GENESIS_WASI_PARITY_HELP"
run_help "$GENESIS_BIN" "$GENESIS_FMT_HELP" fmt
run_help "$GENESIS_WASI_BIN" "$GENESIS_WASI_FMT_HELP" fmt
run_help "$GENESIS_PARITY_BIN" "$GENESIS_PARITY_FMT_HELP" fmt
run_help "$GENESIS_WASI_PARITY_BIN" "$GENESIS_WASI_PARITY_FMT_HELP" fmt

# Production binaries must advertise selfhost-only accepted values.
assert_contains "$GENESIS_HELP" "Accepted value: selfhost."
assert_contains "$GENESIS_WASI_HELP" "Accepted value: selfhost."
assert_contains "$GENESIS_FMT_HELP" "Accepted value: selfhost."
assert_contains "$GENESIS_WASI_FMT_HELP" "Accepted value: selfhost."
assert_contains "$GENESIS_HELP" "Accepted value: artifact-only."
assert_contains "$GENESIS_WASI_HELP" "Accepted value: artifact-only."
assert_not_contains "$GENESIS_HELP" "Accepted values: selfhost, rust."
assert_not_contains "$GENESIS_WASI_HELP" "Accepted values: selfhost, rust."
assert_not_contains "$GENESIS_FMT_HELP" "Accepted values: selfhost, rust."
assert_not_contains "$GENESIS_WASI_FMT_HELP" "Accepted values: selfhost, rust."

# Parity harness binaries must preserve explicit rust compatibility surface.
assert_contains "$GENESIS_PARITY_HELP" "Accepted values: selfhost, rust."
assert_contains "$GENESIS_WASI_PARITY_HELP" "Accepted values: selfhost, rust."
assert_contains "$GENESIS_PARITY_FMT_HELP" "Accepted values: selfhost, rust."
assert_contains "$GENESIS_WASI_PARITY_FMT_HELP" "Accepted values: selfhost, rust."

genesis_profile_gate_emit_runtime_report \
  "production-cli-help-surface" \
  "genesis/production-cli-help-surface-v0.1" \
  "$REPORT_PATH" \
  "$HISTORY_PATH" \
  "$START_MS" \
  "$BUDGET_MS"

echo "help-surface: ok"
