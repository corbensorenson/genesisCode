#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

PATTERN='CoreformFrontend::Rust'
MATCH_OUT="/tmp/genesis_rust_frontend_refs.$$"

run_search() {
  local status=0
  if command -v rg >/dev/null 2>&1; then
    rg -n "$PATTERN" crates/*/src \
      --glob '!**/tests/**' \
      --glob '!**/*_parity.rs' >"$MATCH_OUT" 2>/dev/null || status=$?
  elif command -v grep >/dev/null 2>&1; then
    grep -R -n --include='*.rs' \
      --exclude='*_parity.rs' \
      --exclude-dir='tests' \
      -- "$PATTERN" crates/*/src >"$MATCH_OUT" 2>/dev/null || status=$?
  else
    echo "no-production-rust-frontend-refs: missing required search tools (rg or grep)" >&2
    return 2
  fi

  case "$status" in
    0|1)
      return "$status"
      ;;
    *)
      echo "no-production-rust-frontend-refs: search failed with status $status" >&2
      return 2
      ;;
  esac
}

search_status=0
run_search || search_status=$?

if [[ "$search_status" -eq 0 ]]; then
  echo "no-production-rust-frontend-refs: found forbidden rust-frontend reference(s) in production sources" >&2
  cat "$MATCH_OUT" >&2
  rm -f "$MATCH_OUT" || true
  exit 1
fi
if [[ "$search_status" -ne 1 ]]; then
  rm -f "$MATCH_OUT" || true
  exit 1
fi
rm -f "$MATCH_OUT" || true

echo "no-production-rust-frontend-refs: ok"
