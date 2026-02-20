#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

PATTERN='CoreformFrontend::Rust'
MATCH_OUT="/tmp/genesis_rust_frontend_refs.$$"
if rg -n "$PATTERN" crates/*/src \
  --glob '!**/tests/**' \
  --glob '!**/*_parity.rs' >"$MATCH_OUT" 2>/dev/null; then
  echo "no-production-rust-frontend-refs: found forbidden rust-frontend reference(s) in production sources" >&2
  cat "$MATCH_OUT" >&2
  rm -f "$MATCH_OUT" || true
  exit 1
fi
rm -f "$MATCH_OUT" || true

echo "no-production-rust-frontend-refs: ok"
