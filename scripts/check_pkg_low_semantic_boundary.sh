#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

RUNNER_FILE="crates/gc_effects/src/runner_cap_pkg_low.rs"
RUNNER_MODULE_SEMANTICS_FILE="crates/gc_effects/src/runner_cap_pkg_low/module_semantics.rs"
SPEC_FILE="docs/spec/SELF_HOST_BOUNDARY.md"

file_has_pattern() {
  local pattern="$1"
  shift
  if command -v rg >/dev/null 2>&1; then
    rg -n "$pattern" "$@" >/dev/null
  else
    grep -En "$pattern" "$@" >/dev/null
  fi
}

print_file_matches() {
  local pattern="$1"
  shift
  if command -v rg >/dev/null 2>&1; then
    rg -n "$pattern" "$@" || true
  else
    grep -En "$pattern" "$@" || true
  fi
}

if [[ ! -f "$RUNNER_FILE" ]]; then
  echo "pkg-low-boundary: missing $RUNNER_FILE"
  exit 1
fi

if [[ ! -f "$RUNNER_MODULE_SEMANTICS_FILE" ]]; then
  echo "pkg-low-boundary: missing $RUNNER_MODULE_SEMANTICS_FILE"
  exit 1
fi

if file_has_pattern 'gc_pkg::parse_canonical_module_source\(' "$RUNNER_FILE" "$RUNNER_MODULE_SEMANTICS_FILE"; then
  echo "pkg-low-boundary: temporary gc_pkg module semantic bridge must not be used"
  print_file_matches 'gc_pkg::parse_canonical_module_source\(' "$RUNNER_FILE" "$RUNNER_MODULE_SEMANTICS_FILE"
  exit 1
fi

if file_has_pattern 'Temporary package semantic bridge' "$SPEC_FILE"; then
  echo "pkg-low-boundary: bridge rationale is stale; remove temporary bridge section from $SPEC_FILE"
  exit 1
fi

echo "pkg-low-boundary: ok"
