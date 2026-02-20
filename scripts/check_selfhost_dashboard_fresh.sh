#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

DASHBOARD_MD="$ROOT_DIR/docs/status/SELFHOST_CUTOVER.md"
[[ -f "$DASHBOARD_MD" ]] || {
  echo "selfhost-dashboard-fresh: missing committed dashboard at $DASHBOARD_MD" >&2
  exit 1
}

GENESIS_BIN="$ROOT_DIR/target/debug/genesis"
if [[ ! -x "$GENESIS_BIN" ]]; then
  cargo build -p gc_cli >/dev/null
fi

TMP_DIR="$(mktemp -d)"
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

REBUILT_MD="$TMP_DIR/SELFHOST_CUTOVER.md"
"$GENESIS_BIN" \
  --selfhost-artifact "selfhost/toolchain.gc" \
  selfhost-dashboard \
  --markdown "$REBUILT_MD" \
  >/dev/null

if ! cmp -s "$DASHBOARD_MD" "$REBUILT_MD"; then
  echo "selfhost-dashboard-fresh: committed dashboard is stale." >&2
  echo "  expected: docs/status/SELFHOST_CUTOVER.md matches fresh selfhost-dashboard output" >&2
  echo "  fix: cargo run -p gc_cli -- --selfhost-artifact selfhost/toolchain.gc selfhost-dashboard --markdown docs/status/SELFHOST_CUTOVER.md" >&2
  exit 1
fi

echo "selfhost-dashboard-fresh: ok"
