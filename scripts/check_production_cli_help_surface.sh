#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

run_help() {
  local pkg="$1"
  local bin="$2"
  local out="$3"
  shift 3
  cargo run --release -q -p "$pkg" --bin "$bin" -- "$@" --help >"$out"
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

run_help gc_cli genesis "$GENESIS_HELP"
run_help gc_wasi_cli genesis_wasi "$GENESIS_WASI_HELP"
run_help gc_cli genesis_parity "$GENESIS_PARITY_HELP"
run_help gc_wasi_cli genesis_wasi_parity "$GENESIS_WASI_PARITY_HELP"
run_help gc_cli genesis "$GENESIS_FMT_HELP" fmt
run_help gc_wasi_cli genesis_wasi "$GENESIS_WASI_FMT_HELP" fmt
run_help gc_cli genesis_parity "$GENESIS_PARITY_FMT_HELP" fmt
run_help gc_wasi_cli genesis_wasi_parity "$GENESIS_WASI_PARITY_FMT_HELP" fmt

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

echo "help-surface: ok"
