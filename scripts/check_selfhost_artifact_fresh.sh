#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

REPO_ARTIFACT="$ROOT_DIR/selfhost/toolchain.gc"
[[ -f "$REPO_ARTIFACT" ]] || {
  echo "selfhost-artifact-fresh: missing committed artifact at $REPO_ARTIFACT" >&2
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

REBUILT="$TMP_DIR/toolchain.rebuilt.gc"
"$GENESIS_BIN" selfhost-artifact --out "$REBUILT" >/dev/null

if ! cmp -s "$REPO_ARTIFACT" "$REBUILT"; then
  echo "selfhost-artifact-fresh: committed selfhost/toolchain.gc is stale." >&2
  echo "  expected: byte-for-byte match with a fresh `genesis selfhost-artifact --out ...` build" >&2
  echo "  fix: cargo run -p gc_cli -- selfhost-artifact --out selfhost/toolchain.gc" >&2
  exit 1
fi

echo "selfhost-artifact-fresh: ok"
