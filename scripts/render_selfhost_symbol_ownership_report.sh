#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if [[ "$#" -ne 3 ]]; then
  echo "usage: $0 <report-output> <history-output> <history-input>" >&2
  exit 2
fi

REPORT_PATH="$1"
HISTORY_PATH="$2"
HISTORY_INPUT_PATH="$3"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
source "$ROOT_DIR/scripts/lib/profile_gate_timing.sh"

START_MS="$(genesis_profile_gate_now_ms)"
BUDGET_MS="${GENESIS_SELFHOST_SYMBOL_OWNERSHIP_BUDGET_MS:-300000}"

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
  genesis_configure_cargo_target_dir \
    "$ROOT_DIR" \
    "selfhost-symbol-ownership" \
    root-host
  if [[ -z "$GENESIS_BIN_OVERRIDE" ]]; then
    GENESIS_BIN="$CARGO_TARGET_DIR/debug/genesis"
  fi
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

BASELINE_HISTORY=""
if [[ "$HISTORY_INPUT_PATH" != "$HISTORY_PATH" && -f "$HISTORY_INPUT_PATH" ]]; then
  BASELINE_HISTORY="$HISTORY_INPUT_PATH"
fi

genesis_profile_gate_emit_runtime_report \
  "selfhost-symbol-ownership" \
  "genesis/selfhost-symbol-ownership-v0.1" \
  "$REPORT_PATH" \
  "$HISTORY_PATH" \
  "$START_MS" \
  "$BUDGET_MS" \
  "1" \
  "" \
  "" \
  "$BASELINE_HISTORY"
