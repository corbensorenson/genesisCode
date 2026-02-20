#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

RUNNER_FILE="crates/gc_effects/src/runner_cap_pkg_low.rs"
HELPER_FILE="crates/gc_pkg/src/module_semantics.rs"
SPEC_FILE="docs/spec/SELF_HOST_BOUNDARY.md"

if [[ ! -f "$RUNNER_FILE" ]]; then
  echo "pkg-low-boundary: missing $RUNNER_FILE"
  exit 1
fi

if [[ ! -f "$HELPER_FILE" ]]; then
  echo "pkg-low-boundary: missing $HELPER_FILE"
  exit 1
fi

if rg -n 'parse_module\(|canonicalize_module\(|hash_module\(' "$RUNNER_FILE" >/dev/null; then
  echo "pkg-low-boundary: runner must not call parse/canon/hash directly"
  rg -n 'parse_module\(|canonicalize_module\(|hash_module\(' "$RUNNER_FILE" || true
  exit 1
fi

if ! rg -n 'gc_pkg::parse_canonical_module_source\(' "$RUNNER_FILE" >/dev/null; then
  echo "pkg-low-boundary: runner must route module semantics via gc_pkg::parse_canonical_module_source"
  exit 1
fi

if ! rg -n 'Temporary package semantic bridge' "$SPEC_FILE" >/dev/null; then
  echo "pkg-low-boundary: expected boundary rationale section missing in $SPEC_FILE"
  exit 1
fi

echo "pkg-low-boundary: ok"
