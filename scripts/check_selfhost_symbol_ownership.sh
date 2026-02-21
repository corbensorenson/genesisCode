#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

GENESIS_BIN="${GENESIS_BIN:-$ROOT_DIR/target/debug/genesis}"
if [[ ! -x "$GENESIS_BIN" ]]; then
  cargo build -p gc_cli >/dev/null
fi

TMP_JSON="$(mktemp)"
trap 'rm -f "$TMP_JSON"' EXIT

"$GENESIS_BIN" --json agent-index >"$TMP_JSON"

python3 - "$TMP_JSON" <<'PY'
import json
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
doc = json.loads(path.read_text(encoding="utf-8"))
if doc.get("kind") != "genesis/agent-index-v0.1":
    raise SystemExit("selfhost-symbol-ownership: unexpected agent-index response kind")

idx = doc.get("data", {}).get("selfhost_symbol_index", {})
if idx.get("loaded") is not True:
    raise SystemExit(
        "selfhost-symbol-ownership: selfhost symbol index failed to load: "
        + str(idx.get("error", "unknown error"))
    )

unresolved = idx.get("unresolved_required_symbols") or []
duplicates = idx.get("duplicate_symbol_owners") or []
if unresolved:
    raise SystemExit(
        "selfhost-symbol-ownership: unresolved required symbols: "
        + ", ".join(str(x) for x in unresolved)
    )
if duplicates:
    msgs = []
    for entry in duplicates:
        symbol = entry.get("symbol", "<unknown>")
        module_paths = entry.get("module_paths", [])
        msgs.append(f"{symbol} -> {module_paths}")
    raise SystemExit(
        "selfhost-symbol-ownership: duplicate symbol owners detected: "
        + "; ".join(msgs)
    )

print(
    "selfhost-symbol-ownership: ok "
    f"(symbols={idx.get('symbol_count', 0)} required={idx.get('required_symbol_count', 0)})"
)
PY
