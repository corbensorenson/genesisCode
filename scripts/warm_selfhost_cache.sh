#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

GENESIS_BIN="$ROOT_DIR/target/debug/genesis"
if [[ ! -x "$GENESIS_BIN" ]]; then
  cargo build -p gc_cli >/dev/null
fi

CACHE_ARTIFACT="${GENESIS_WARM_SELFHOST_ARTIFACT:-$ROOT_DIR/.genesis/cache/selfhost/toolchain.gc}"
mkdir -p "$(dirname "$CACHE_ARTIFACT")"

needs_rebuild=1
if [[ -f "$CACHE_ARTIFACT" ]]; then
  needs_rebuild=0
  while IFS= read -r src; do
    if [[ "$src" -nt "$CACHE_ARTIFACT" ]]; then
      needs_rebuild=1
      break
    fi
  done < <(find "$ROOT_DIR/selfhost" "$ROOT_DIR/prelude" -type f \( -name '*.gc' -o -name '*.toml' \) -print | sort)
fi

if (( needs_rebuild == 1 )); then
  "$GENESIS_BIN" selfhost-artifact --out "$CACHE_ARTIFACT" >/dev/null
fi

echo "selfhost-cache: ok artifact=$CACHE_ARTIFACT rebuilt=$needs_rebuild"
