#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if [[ $# -ne 2 || -z "${1:-}" || -z "${2:-}" ]]; then
  echo "usage: scripts/render_selfhost_artifact_fresh_report.sh <report-path> <history-path>" >&2
  exit 2
fi

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
source "$ROOT_DIR/scripts/lib/profile_gate_timing.sh"

START_MS="$(genesis_profile_gate_now_ms)"
REPORT_PATH="$1"
HISTORY_PATH="$2"
BUDGET_MS="${GENESIS_SELFHOST_ARTIFACT_FRESH_BUDGET_MS:-900000}"

REPO_ARTIFACT="$ROOT_DIR/selfhost/toolchain.gc"
MANIFEST_FILE="$ROOT_DIR/selfhost/toolchain_manifest.gc"
FRESHNESS_FILE="$ROOT_DIR/selfhost/toolchain.freshness.json"
DISK_MIN_FREE_KB="${GENESIS_SELFHOST_ARTIFACT_FRESH_MIN_FREE_KB:-1048576}"
DISK_STRICT_MODE="${GENESIS_SELFHOST_ARTIFACT_FRESH_DISK_STRICT_MODE:-1}"
[[ -f "$REPO_ARTIFACT" ]] || {
  echo "selfhost-artifact-fresh: missing committed artifact at $REPO_ARTIFACT" >&2
  exit 1
}
[[ -f "$MANIFEST_FILE" ]] || {
  echo "selfhost-artifact-fresh: missing selfhost manifest at $MANIFEST_FILE" >&2
  exit 1
}

compute_source_hash() {
  python3 - "$ROOT_DIR" "$MANIFEST_FILE" <<'PY'
import hashlib
import pathlib
import re
import sys

root = pathlib.Path(sys.argv[1]).resolve()
manifest = pathlib.Path(sys.argv[2]).resolve()
text = manifest.read_text(encoding="utf-8")
paths = sorted(set(re.findall(r'"(selfhost/[A-Za-z0-9_./-]+\.gc)"', text)))
if not paths:
    raise SystemExit("selfhost-artifact-fresh: no module paths found in selfhost/toolchain_manifest.gc")

h = hashlib.sha256()
h.update(b"manifest\0")
h.update(str(manifest.relative_to(root)).encode("utf-8"))
h.update(b"\0")
h.update(manifest.read_bytes())

for rel in paths:
    p = root / rel
    if not p.is_file():
        raise SystemExit(f"selfhost-artifact-fresh: manifest module missing: {rel}")
    h.update(b"\0module\0")
    h.update(rel.encode("utf-8"))
    h.update(b"\0")
    h.update(p.read_bytes())
print(h.hexdigest())
PY
}

artifact_hash() {
  python3 - "$1" <<'PY'
import hashlib
import pathlib
import sys
p = pathlib.Path(sys.argv[1])
print(hashlib.sha256(p.read_bytes()).hexdigest())
PY
}

SOURCE_HASH="$(compute_source_hash)"
ART_HASH="$(artifact_hash "$REPO_ARTIFACT")"

emit_runtime_report() {
  genesis_profile_gate_emit_runtime_report \
    "selfhost-artifact-fresh" \
    "genesis/selfhost-artifact-fresh-v0.1" \
    "$REPORT_PATH" \
    "$HISTORY_PATH" \
    "$START_MS" \
    "$BUDGET_MS"
}

if [[ -f "$FRESHNESS_FILE" ]]; then
  if python3 - "$FRESHNESS_FILE" "$SOURCE_HASH" "$ART_HASH" <<'PY'
import json
import pathlib
import sys

meta_path = pathlib.Path(sys.argv[1])
source_hash = sys.argv[2]
artifact_hash = sys.argv[3]
try:
    meta = json.loads(meta_path.read_text(encoding="utf-8"))
except Exception:
    raise SystemExit(1)
ok = (
    isinstance(meta, dict)
    and meta.get("kind") == "genesis/selfhost-freshness-v0.1"
    and meta.get("source_hash_sha256") == source_hash
    and meta.get("artifact_hash_sha256") == artifact_hash
)
if not ok:
    raise SystemExit(1)
PY
  then
    emit_runtime_report
    echo "selfhost-artifact-fresh: ok (fast-path metadata match)"
    exit 0
  fi
fi

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
    --context "selfhost-artifact-fresh" \
    --min-kb "$DISK_MIN_FREE_KB" \
    --strict "$DISK_STRICT_MODE"
  genesis_configure_cargo_target_dir \
    "$ROOT_DIR" \
    "selfhost-artifact-fresh" \
    root-host
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

REBUILT="$TMP_DIR/toolchain.rebuilt.gc"
"$GENESIS_BIN" selfhost-artifact --out "$REBUILT" >/dev/null

if ! cmp -s "$REPO_ARTIFACT" "$REBUILT"; then
  echo "selfhost-artifact-fresh: committed selfhost/toolchain.gc is stale." >&2
  echo "  expected: byte-for-byte match with a fresh 'genesis selfhost-artifact --out ...' build" >&2
  echo "  fix: cargo run -p gc_cli -- selfhost-artifact --out selfhost/toolchain.gc" >&2
  echo "  then: bash scripts/update_selfhost_freshness_metadata.sh" >&2
  exit 1
fi

echo "selfhost-artifact-fresh: artifact bytes match, but committed freshness metadata is missing or stale." >&2
echo "  fix: bash scripts/update_selfhost_freshness_metadata.sh" >&2
echo "  then rerun: bash scripts/check_selfhost_artifact_fresh.sh" >&2
exit 1
