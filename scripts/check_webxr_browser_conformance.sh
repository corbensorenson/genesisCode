#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

OUT="${GENESIS_WEBXR_BROWSER_CONFORMANCE_OUT:-.genesis/perf/webxr_browser_conformance_report.json}"
TIMEOUT_MS="${GENESIS_WEBXR_BROWSER_CONFORMANCE_TIMEOUT_MS:-8000}"

[[ "$TIMEOUT_MS" =~ ^[0-9]+$ && "$TIMEOUT_MS" -gt 0 ]] || {
  echo "webxr-browser-conformance: GENESIS_WEBXR_BROWSER_CONFORMANCE_TIMEOUT_MS must be positive integer" >&2
  exit 2
}

mkdir -p "$(dirname "$OUT")"
echo "webxr-browser-conformance: running browser-native WebXR conformance lane"
GENESIS_WEBXR_BROWSER_CONFORMANCE_OUT="$OUT" \
GENESIS_WEBXR_BROWSER_CONFORMANCE_TIMEOUT_MS="$TIMEOUT_MS" \
node scripts/webxr_browser_conformance.mjs

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
if frame.get("status") not in {"ok", "timeout", "error"}:
    raise SystemExit(
        f"webxr-browser-conformance: invalid frame status {frame.get('status')!r}"
    )

print(f"webxr-browser-conformance: ok report={report_path}")
PY
