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
  cargo run --release -q -p "$pkg" --bin "$bin" -- --help >"$out"
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

run_help gc_cli genesis "$GENESIS_HELP"
run_help gc_wasi_cli genesis_wasi "$GENESIS_WASI_HELP"
run_help gc_cli genesis_parity "$GENESIS_PARITY_HELP"
run_help gc_wasi_cli genesis_wasi_parity "$GENESIS_WASI_PARITY_HELP"

# Production binaries must not advertise rust parse-surface values.
assert_not_contains "$GENESIS_HELP" "[possible values: selfhost, rust]"
assert_not_contains "$GENESIS_WASI_HELP" "[possible values: selfhost, rust]"

# Parity harness binaries must preserve explicit rust compatibility surface.
assert_contains "$GENESIS_PARITY_HELP" "[possible values: selfhost, rust]"
assert_contains "$GENESIS_WASI_PARITY_HELP" "[possible values: selfhost, rust]"

echo "help-surface: ok"
