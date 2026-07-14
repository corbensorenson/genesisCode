#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

python3 scripts/lib/roadmap_execution_manifest.py --self-test >/dev/null
python3 scripts/lib/roadmap_execution_manifest.py \
  --render \
  --output "$TMP_DIR/rendered.json" >/dev/null

if ! cmp -s docs/program/ROADMAP_EXECUTION_MANIFEST_v0.1.json "$TMP_DIR/rendered.json"; then
  echo "roadmap-execution-manifest: generated manifest drift" >&2
  echo "roadmap-execution-manifest: run bash scripts/update_roadmap_execution_manifest.sh" >&2
  exit 1
fi

cp policies/roadmap_execution_v0.1.json "$TMP_DIR/duplicate-key-policy.json"
python3 - "$TMP_DIR/duplicate-key-policy.json" <<'PY'
from pathlib import Path
import sys

path = Path(sys.argv[1])
text = path.read_text(encoding="utf-8")
needle = '  "version": "0.1",\n'
if text.count(needle) != 1:
    raise SystemExit("roadmap-execution-manifest: duplicate-key fixture anchor drift")
path.write_text(text.replace(needle, needle + needle, 1), encoding="utf-8")
PY
if python3 scripts/lib/roadmap_execution_manifest.py \
  --render \
  --policy "$TMP_DIR/duplicate-key-policy.json" \
  --output "$TMP_DIR/rejected.json" >/dev/null 2>&1; then
  echo "roadmap-execution-manifest: duplicate-key policy fixture was accepted" >&2
  exit 1
fi

before="$(cksum docs/program/ROADMAP_EXECUTION_MANIFEST_v0.1.json)"
python3 scripts/lib/roadmap_execution_manifest.py --check
after="$(cksum docs/program/ROADMAP_EXECUTION_MANIFEST_v0.1.json)"
[[ "$before" == "$after" ]] || {
  echo "roadmap-execution-manifest: check mode mutated the retained manifest" >&2
  exit 1
}

echo "roadmap-execution-manifest-contract: ok (negative_controls=14 check_mode=read_only)"
