#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

export LC_ALL=C

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

HELPER_FILE="scripts/lib/capability_dispatch_ops.sh"
FIXTURE_DIR="tests/spec/guard_fixtures"

if [[ ! -f "$HELPER_FILE" ]]; then
  echo "guard-extraction-fixtures: missing helper file: $HELPER_FILE"
  exit 1
fi
if [[ ! -d "$FIXTURE_DIR" ]]; then
  echo "guard-extraction-fixtures: missing fixture dir: $FIXTURE_DIR"
  exit 1
fi

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

# shellcheck source=scripts/lib/capability_dispatch_ops.sh
source "$HELPER_FILE"

check_fixture() {
  local fixture="$1"
  local expected="$2"
  local actual="$TMP_DIR/$(basename "$fixture").actual"

  if [[ ! -f "$fixture" ]]; then
    echo "guard-extraction-fixtures: missing fixture: $fixture"
    exit 1
  fi
  if [[ ! -f "$expected" ]]; then
    echo "guard-extraction-fixtures: missing expected file: $expected"
    exit 1
  fi

  extract_call_capability_ops "$fixture" >"$actual"
  if ! diff -u "$expected" "$actual" >/dev/null; then
    echo "guard-extraction-fixtures: extraction mismatch for fixture: $fixture"
    diff -u "$expected" "$actual" || true
    exit 1
  fi
}

check_fixture \
  "$FIXTURE_DIR/call_capability_match_op.rs" \
  "$FIXTURE_DIR/call_capability_match_op.expected"
check_fixture \
  "$FIXTURE_DIR/call_capability_match_op_eff.rs" \
  "$FIXTURE_DIR/call_capability_match_op_eff.expected"

echo "guard-extraction-fixtures: ok"
