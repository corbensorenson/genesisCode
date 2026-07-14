#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

tmp="$(mktemp "${TMPDIR:-/tmp}/gc-agent-symbol-index.XXXXXX")"
trap 'rm -f "$tmp"' EXIT
python3 scripts/lib/gc_agent_symbol_index.py --render >"$tmp"
mv "$tmp" docs/spec/GC_AGENT_SYMBOL_INDEX_v0.3.json
trap - EXIT
echo "gc-agent-symbol-index: updated docs/spec/GC_AGENT_SYMBOL_INDEX_v0.3.json"
