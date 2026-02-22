#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"

DASHBOARD_MD="$ROOT_DIR/docs/status/SELFHOST_CUTOVER.md"
[[ -f "$DASHBOARD_MD" ]] || {
  echo "selfhost-dashboard-fresh: missing committed dashboard at $DASHBOARD_MD" >&2
  exit 1
}
DISK_MIN_FREE_KB="${GENESIS_SELFHOST_DASHBOARD_FRESH_MIN_FREE_KB:-1048576}"
DISK_STRICT_MODE="${GENESIS_SELFHOST_DASHBOARD_FRESH_DISK_STRICT_MODE:-1}"
GENESIS_BIN_OVERRIDE="${GENESIS_BIN:-}"

DEFAULT_DEBUG_DIR="$ROOT_DIR/target/debug"
if [[ -n "${CARGO_TARGET_DIR:-}" ]]; then
  DEFAULT_DEBUG_DIR="$CARGO_TARGET_DIR/debug"
fi
if [[ -n "$GENESIS_BIN_OVERRIDE" ]]; then
  GENESIS_BIN="$GENESIS_BIN_OVERRIDE"
else
  GENESIS_BIN="$DEFAULT_DEBUG_DIR/genesis"
fi
if [[ ! -x "$GENESIS_BIN" ]]; then
  bash scripts/check_disk_headroom.sh \
    --path "$ROOT_DIR" \
    --context "selfhost-dashboard-fresh" \
    --min-kb "$DISK_MIN_FREE_KB" \
    --strict "$DISK_STRICT_MODE"
  genesis_configure_cargo_target_dir \
    "$ROOT_DIR" \
    "selfhost-dashboard-fresh" \
    ".genesis/build/selfhost_dashboard_fresh" \
    "GENESIS_SELFHOST_DASHBOARD_FRESH_CARGO_TARGET_DIR"
  if [[ -z "$GENESIS_BIN_OVERRIDE" ]]; then
    GENESIS_BIN="$CARGO_TARGET_DIR/debug/genesis"
  fi
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
