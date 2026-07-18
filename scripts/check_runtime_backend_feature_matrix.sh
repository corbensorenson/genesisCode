#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

TIMING_BASELINE_FILE="${GENESIS_CHECK_RUNTIME_BACKEND_MATRIX_HISTORY_INPUT:-.genesis/perf/runtime_backend_feature_matrix_history.jsonl}"
PREBUILT_REPORT="${GENESIS_CHECK_RUNTIME_BACKEND_MATRIX_REPORT:-}"
PREBUILT_MANIFEST="${GENESIS_CHECK_RUNTIME_BACKEND_MATRIX_MANIFEST:-}"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

if [[ -n "$PREBUILT_REPORT" ]]; then
  if [[ -z "$PREBUILT_MANIFEST" ]]; then
    echo "runtime-backend-feature-matrix: prebuilt report requires GENESIS_CHECK_RUNTIME_BACKEND_MATRIX_MANIFEST" >&2
    exit 2
  fi
  python3 - "$PREBUILT_REPORT" "$PREBUILT_MANIFEST" <<'PY'
import hashlib
import json
import pathlib
import sys

path = pathlib.Path(sys.argv[1]).resolve(strict=True)
manifest_path = pathlib.Path(sys.argv[2]).resolve(strict=True)
if manifest_path.name != "manifest.json" or path.parent != manifest_path.parent:
    raise SystemExit(
        "runtime-backend-feature-matrix: prebuilt report and manifest must be direct siblings"
    )
payload = path.read_bytes()
doc = json.loads(payload)
if doc.get("kind") != "genesis/runtime-backend-feature-matrix-v0.1":
    raise SystemExit("runtime-backend-feature-matrix: prebuilt report kind mismatch")
if doc.get("ok") is not True:
    raise SystemExit("runtime-backend-feature-matrix: prebuilt report has ok=false")
manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
if manifest.get("kind") != "genesis/health-profile-evidence-bundle-v0.1":
    raise SystemExit("runtime-backend-feature-matrix: prebuilt manifest kind mismatch")
if manifest.get("ok") is not True or manifest.get("profile") != "release-full":
    raise SystemExit("runtime-backend-feature-matrix: prebuilt manifest profile mismatch")
entry = manifest.get("evidence", {}).get(path.name)
if not isinstance(entry, dict):
    raise SystemExit("runtime-backend-feature-matrix: report absent from prebuilt manifest")
if entry.get("kind") != doc.get("kind"):
    raise SystemExit("runtime-backend-feature-matrix: prebuilt manifest report kind mismatch")
if entry.get("sha256") != hashlib.sha256(payload).hexdigest():
    raise SystemExit("runtime-backend-feature-matrix: prebuilt report hash mismatch")
print(f"runtime-backend-feature-matrix: prebuilt report ok ({path})")
PY
  exit 0
fi

exec bash scripts/render_runtime_backend_feature_matrix_report.sh \
  "$TMP_DIR/runtime_backend_feature_matrix_report.json" \
  "$TMP_DIR/runtime_backend_feature_matrix_history.jsonl" \
  "$TIMING_BASELINE_FILE"
