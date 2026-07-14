#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/genesis-evidence-profile.XXXXXX")"
trap 'rm -rf "$TMP_DIR"' EXIT

python3 scripts/lib/genesis_evidence_profile.py \
  --render \
  --output "$TMP_DIR/rendered.json"

if ! cmp -s docs/program/evidence/GENESIS_EVIDENCE_BUNDLE_v0.1.json "$TMP_DIR/rendered.json"; then
  echo "genesis-evidence-profile: retained vector drift" >&2
  echo "genesis-evidence-profile: run bash scripts/update_genesis_evidence_profile.sh" >&2
  exit 1
fi

cp docs/program/evidence/GENESIS_EVIDENCE_BUNDLE_v0.1.json "$TMP_DIR/duplicate-key.json"
python3 - "$TMP_DIR/duplicate-key.json" <<'PY'
from pathlib import Path
import sys

path = Path(sys.argv[1])
text = path.read_text(encoding="utf-8")
needle = '  "profile": "E3",\n  "version": "0.1"\n'
if text.count(needle) != 1:
    raise SystemExit("genesis-evidence-profile: duplicate-key fixture anchor drift")
replacement = '  "profile": "E3",\n  "version": "0.1",\n  "version": "0.1"\n'
path.write_text(text.replace(needle, replacement, 1), encoding="utf-8")
PY
if python3 scripts/lib/genesis_evidence_profile.py \
  --check \
  --input "$TMP_DIR/duplicate-key.json" >/dev/null 2>&1; then
  echo "genesis-evidence-profile: duplicate JSON key was accepted" >&2
  exit 1
fi

before="$(cksum docs/program/evidence/GENESIS_EVIDENCE_BUNDLE_v0.1.json)"
python3 scripts/lib/genesis_evidence_profile.py --check
after="$(cksum docs/program/evidence/GENESIS_EVIDENCE_BUNDLE_v0.1.json)"
[[ "$before" == "$after" ]] || {
  echo "genesis-evidence-profile: check mode mutated retained evidence" >&2
  exit 1
}

echo "genesis-evidence-profile-contract: ok (check_mode=read_only duplicate_keys=rejected)"
