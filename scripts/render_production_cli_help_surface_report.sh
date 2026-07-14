#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if [[ "$#" -ne 3 ]]; then
  echo "usage: $0 <report-output> <history-output> <history-input>" >&2
  exit 2
fi

REPORT_PATH="$1"
HISTORY_PATH="$2"
RETAINED_HISTORY_INPUT="$3"

source "$ROOT/scripts/lib/cargo_target_dir.sh"
source "$ROOT/scripts/lib/profile_gate_timing.sh"
source "$ROOT/scripts/lib/release_bin.sh"
source "$ROOT/scripts/lib/heavy_gate_preflight.sh"
genesis_configure_cargo_target_dir \
  "$ROOT" \
  "check-production-cli-help-surface" \
  root-host

DISK_MIN_FREE_KB="${GENESIS_PRODUCTION_CLI_HELP_SURFACE_MIN_FREE_KB:-2097152}"
DISK_AUTO_RECLAIM="${GENESIS_PRODUCTION_CLI_HELP_SURFACE_DISK_AUTO_RECLAIM:-0}"
TMP_ROOT="${GENESIS_PRODUCTION_CLI_HELP_SURFACE_TMPDIR:-$ROOT/.genesis/tmp/check-production-cli-help-surface}"
genesis_heavy_gate_preflight \
  "$ROOT" \
  "production-cli-help-surface" \
  "$DISK_MIN_FREE_KB" \
  "$TMP_ROOT" \
  "$DISK_AUTO_RECLAIM"

START_MS="$(genesis_profile_gate_now_ms)"
BUDGET_MS="${GENESIS_PRODUCTION_CLI_HELP_SURFACE_BUDGET_MS:-240000}"
HISTORY_SCOPE_KEY="${GENESIS_PRODUCTION_CLI_HELP_SURFACE_HISTORY_SCOPE_KEY:-production-only-v1}"
BASELINE_HISTORY_PATH="${GENESIS_PRODUCTION_CLI_HELP_SURFACE_BASELINE_HISTORY:-policies/perf/production_cli_help_surface_seed_history.jsonl}"
MIN_HISTORY="${GENESIS_PRODUCTION_CLI_HELP_SURFACE_MIN_HISTORY:-5}"
REQUIRE_MIN_HISTORY="${GENESIS_PRODUCTION_CLI_HELP_SURFACE_REQUIRE_MIN_HISTORY:-0}"
INCLUDE_PARITY="${GENESIS_PRODUCTION_CLI_HELP_SURFACE_INCLUDE_PARITY:-0}"

if [[ "$INCLUDE_PARITY" != "0" && "$INCLUDE_PARITY" != "1" ]]; then
  echo "help-surface: GENESIS_PRODUCTION_CLI_HELP_SURFACE_INCLUDE_PARITY must be 0 or 1" >&2
  exit 2
fi
if [[ ! "$MIN_HISTORY" =~ ^[0-9]+$ || "$MIN_HISTORY" -le 0 ]]; then
  echo "help-surface: GENESIS_PRODUCTION_CLI_HELP_SURFACE_MIN_HISTORY must be a positive integer" >&2
  exit 2
fi
if [[ "$REQUIRE_MIN_HISTORY" != "0" && "$REQUIRE_MIN_HISTORY" != "1" ]]; then
  echo "help-surface: GENESIS_PRODUCTION_CLI_HELP_SURFACE_REQUIRE_MIN_HISTORY must be 0 or 1" >&2
  exit 2
fi

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
GENESIS_FMT_HELP="$TMP/genesis_fmt.help"
GENESIS_WASI_FMT_HELP="$TMP/genesis_wasi_fmt.help"
GENESIS_PARITY_HELP="$TMP/genesis_parity.help"
GENESIS_WASI_PARITY_HELP="$TMP/genesis_wasi_parity.help"
GENESIS_PARITY_FMT_HELP="$TMP/genesis_parity_fmt.help"
GENESIS_WASI_PARITY_FMT_HELP="$TMP/genesis_wasi_parity_fmt.help"

if [[ "$INCLUDE_PARITY" == "1" ]]; then
  # Build production + parity bins in one release invocation when parity validation is explicitly requested.
  genesis_build_release_bins \
    -p gc_cli --bin genesis --bin genesis_parity \
    -p gc_wasi_cli --bin genesis_wasi --bin genesis_wasi_parity
else
  # Default/common lanes validate production bins only to avoid parity-only compile overhead.
  genesis_build_release_bins \
    -p gc_cli --bin genesis \
    -p gc_wasi_cli --bin genesis_wasi
fi

GENESIS_BIN="$(genesis_assert_release_bin genesis)"
GENESIS_WASI_BIN="$(genesis_assert_release_bin genesis_wasi)"

run_help "$GENESIS_BIN" "$GENESIS_HELP"
run_help "$GENESIS_WASI_BIN" "$GENESIS_WASI_HELP"
run_help "$GENESIS_BIN" "$GENESIS_FMT_HELP" fmt
run_help "$GENESIS_WASI_BIN" "$GENESIS_WASI_FMT_HELP" fmt

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

checked_bins_json='["genesis","genesis_wasi"]'
if [[ "$INCLUDE_PARITY" == "1" ]]; then
  GENESIS_PARITY_BIN="$(genesis_assert_release_bin genesis_parity)"
  GENESIS_WASI_PARITY_BIN="$(genesis_assert_release_bin genesis_wasi_parity)"
  run_help "$GENESIS_PARITY_BIN" "$GENESIS_PARITY_HELP"
  run_help "$GENESIS_WASI_PARITY_BIN" "$GENESIS_WASI_PARITY_HELP"
  run_help "$GENESIS_PARITY_BIN" "$GENESIS_PARITY_FMT_HELP" fmt
  run_help "$GENESIS_WASI_PARITY_BIN" "$GENESIS_WASI_PARITY_FMT_HELP" fmt

  # Parity harness binaries must preserve explicit rust compatibility surface.
  assert_contains "$GENESIS_PARITY_HELP" "Accepted values: selfhost, rust."
  assert_contains "$GENESIS_WASI_PARITY_HELP" "Accepted values: selfhost, rust."
  assert_contains "$GENESIS_PARITY_FMT_HELP" "Accepted values: selfhost, rust."
  assert_contains "$GENESIS_WASI_PARITY_FMT_HELP" "Accepted values: selfhost, rust."
  checked_bins_json='["genesis","genesis_wasi","genesis_parity","genesis_wasi_parity"]'
fi

EFFECTIVE_BASELINE_HISTORY="$BASELINE_HISTORY_PATH"
if [[ "$RETAINED_HISTORY_INPUT" != "$HISTORY_PATH" && -f "$RETAINED_HISTORY_INPUT" ]]; then
  EFFECTIVE_BASELINE_HISTORY="$TMP/combined_baseline_history.jsonl"
  cat "$BASELINE_HISTORY_PATH" "$RETAINED_HISTORY_INPUT" >"$EFFECTIVE_BASELINE_HISTORY"
fi

genesis_profile_gate_emit_runtime_report \
  "production-cli-help-surface" \
  "genesis/production-cli-help-surface-v0.1" \
  "$REPORT_PATH" \
  "$HISTORY_PATH" \
  "$START_MS" \
  "$BUDGET_MS" \
  "$MIN_HISTORY" \
  "{\"build_strategy\":\"single-cargo-build\",\"include_parity\":$INCLUDE_PARITY,\"checked_bins\":$checked_bins_json}" \
  "$HISTORY_SCOPE_KEY" \
  "$EFFECTIVE_BASELINE_HISTORY" \
  "$REQUIRE_MIN_HISTORY"

echo "help-surface: ok"
