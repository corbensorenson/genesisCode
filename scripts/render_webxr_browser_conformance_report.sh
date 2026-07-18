#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

if [[ "$#" -ne 1 ]]; then
  echo "usage: $0 <report-output>" >&2
  exit 2
fi

OUT="$1"
TIMEOUT_MS="${GENESIS_WEBXR_BROWSER_CONFORMANCE_TIMEOUT_MS:-8000}"
NODE_BIN="${GENESIS_WEBXR_NODE_BIN:-}"

[[ "$TIMEOUT_MS" =~ ^[0-9]+$ && "$TIMEOUT_MS" -gt 0 ]] || {
  echo "webxr-browser-conformance: GENESIS_WEBXR_BROWSER_CONFORMANCE_TIMEOUT_MS must be positive integer" >&2
  exit 2
}

mkdir -p "$(dirname "$OUT")"

node_22_binary() {
  local candidate="$1"
  local resolved version major
  resolved="$(command -v "$candidate" 2>/dev/null || true)"
  [[ -n "$resolved" && -x "$resolved" ]] || return 1
  version="$("$resolved" --version 2>/dev/null || true)"
  major="${version#v}"
  major="${major%%.*}"
  [[ "$major" == "22" ]] || return 1
  printf '%s\n' "$resolved"
}

if [[ -n "$NODE_BIN" ]]; then
  NODE_BIN="$(node_22_binary "$NODE_BIN")" || {
    echo "webxr-browser-conformance: GENESIS_WEBXR_NODE_BIN must resolve to Node.js 22.x" >&2
    exit 2
  }
else
  for candidate in \
    node \
    node22 \
    /opt/homebrew/opt/node@22/bin/node \
    /usr/local/opt/node@22/bin/node \
    "$HOME"/.nvm/versions/node/v22*/bin/node; do
    if NODE_BIN="$(node_22_binary "$candidate")"; then
      break
    fi
    NODE_BIN=""
  done
  if [[ -z "$NODE_BIN" ]]; then
    echo "webxr-browser-conformance: Node.js 22.x is required by genesis.prerequisites.json" >&2
    exit 2
  fi
fi

echo "webxr-browser-conformance: node=$NODE_BIN version=$("$NODE_BIN" --version)"
echo "webxr-browser-conformance: running browser-native WebXR conformance lane"
GENESIS_WEBXR_BROWSER_CONFORMANCE_OUT="$OUT" \
GENESIS_WEBXR_BROWSER_CONFORMANCE_TIMEOUT_MS="$TIMEOUT_MS" \
"$NODE_BIN" scripts/webxr_browser_conformance.mjs

python3 - "$OUT" <<'PY'
import json
import pathlib
import sys

report_path = pathlib.Path(sys.argv[1])
if not report_path.is_file():
    raise SystemExit(f"webxr-browser-conformance: missing report {report_path}")
doc = json.loads(report_path.read_text(encoding="utf-8"))

if doc.get("kind") != "genesis/webxr-browser-conformance-v0.1":
    raise SystemExit(
        f"webxr-browser-conformance: unexpected report kind {doc.get('kind')!r}"
    )
if doc.get("ok") is not True:
    raise SystemExit("webxr-browser-conformance: report indicates failure")
if doc.get("functional_pass") is not True:
    raise SystemExit("webxr-browser-conformance: functional pass must be true")

cap = doc.get("run_a_capture")
if not isinstance(cap, dict):
    raise SystemExit("webxr-browser-conformance: missing run_a_capture")

session = cap.get("session", {})
if session.get("status") != "opened":
    raise SystemExit("webxr-browser-conformance: expected opened inline session")

input_snapshot = cap.get("input", {})
if input_snapshot.get("status") != "ok":
    raise SystemExit("webxr-browser-conformance: input snapshot must be ok")

haptics = cap.get("haptics", {})
if haptics.get("status") not in {"ok", "error"}:
    raise SystemExit(
        f"webxr-browser-conformance: invalid haptics status {haptics.get('status')!r}"
    )

frame = cap.get("frame", {})
if frame.get("status") != "ok":
    raise SystemExit(
        f"webxr-browser-conformance: expected functional frame status 'ok', got {frame.get('status')!r}"
    )

session_close = cap.get("session_close", {})
if session_close.get("status") not in {"closed", "closed-quiesced"}:
    raise SystemExit(
        f"webxr-browser-conformance: expected closed session status, got {session_close.get('status')!r}"
    )

print(f"webxr-browser-conformance: ok report={report_path}")
PY
