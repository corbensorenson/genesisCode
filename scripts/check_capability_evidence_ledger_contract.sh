#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/genesis-capability-ledger.XXXXXX")"
trap 'rm -rf "$TMP_DIR"' EXIT

LEDGER="docs/spec/CAPABILITY_EVIDENCE_LEDGER_v0.1.json"
MATRIX="$TMP_DIR/feature_matrix.md"
EVIDENCE_JSON="$TMP_DIR/evidence.json"
EVIDENCE_MD="$TMP_DIR/evidence.md"
SELFHOST_STATUS="$TMP_DIR/selfhost-authority.md"
REDTEAM_STATUS="$TMP_DIR/redteam.md"
OUTPUTS=(
  "$MATRIX"
  "$EVIDENCE_JSON"
  "$EVIDENCE_MD"
  "$SELFHOST_STATUS"
  "$REDTEAM_STATUS"
)

run_tool() {
  GENESIS_FEATURE_MATRIX_PATH="$MATRIX" \
  GENESIS_FEATURE_MATRIX_EVIDENCE_JSON="$EVIDENCE_JSON" \
  GENESIS_FEATURE_MATRIX_EVIDENCE_MD="$EVIDENCE_MD" \
  GENESIS_SELFHOST_AUTHORITY_STATUS="$SELFHOST_STATUS" \
  GENESIS_REDTEAM_REPORT_FILE="$REDTEAM_STATUS" \
    python3 scripts/lib/capability_ledger.py "$@"
}

expect_rejected() {
  local label="$1"
  local ledger_path="$2"
  if run_tool --check --ledger "$ledger_path" >"$TMP_DIR/$label.out" 2>&1; then
    echo "capability-evidence-ledger-contract: expected rejection: $label" >&2
    exit 1
  fi
}

run_tool --update --ledger "$LEDGER" >/dev/null
run_tool --check --ledger "$LEDGER" >/dev/null

before="$(cksum "${OUTPUTS[@]}")"
run_tool --check --ledger "$LEDGER" >/dev/null
after="$(cksum "${OUTPUTS[@]}")"
[[ "$before" == "$after" ]] || {
  echo "capability-evidence-ledger-contract: check mode mutated generated views" >&2
  exit 1
}

python3 - "$LEDGER" "$TMP_DIR" <<'PY'
import copy
import json
from pathlib import Path
import sys

source = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
out = Path(sys.argv[2])

def write(name, mutate):
    doc = copy.deepcopy(source)
    mutate(doc)
    (out / name).write_text(json.dumps(doc, indent=2) + "\n", encoding="utf-8")

write("duplicate-id.json", lambda d: d["claims"][1].__setitem__("id", d["claims"][0]["id"]))
write("missing-path.json", lambda d: d["claims"][0]["check_paths"].__setitem__(0, "scripts/does-not-exist.sh"))

def invalid_l5(doc):
    claim = doc["claims"][0]
    claim["maturity"] = "L5"
    for platform in doc["platforms"]:
        claim["maturity_by_platform"][platform["id"]] = "L5"
    claim["immutable_evidence_ids"] = []

write("l5-without-immutable-evidence.json", invalid_l5)
write("unknown-gap.json", lambda d: d["claims"][0]["gap_ids"].append("R99.99.z"))
write("unknown-field.json", lambda d: d["claims"][0].__setitem__("trust_me", True))

raw = Path(sys.argv[1]).read_text(encoding="utf-8")
audit_date = source["audit_date"]
audit_line = f'"audit_date": "{audit_date}",'
(out / "duplicate-key.json").write_text(
    raw.replace(
        audit_line,
        f'{audit_line}\n  {audit_line}',
        1,
    ),
    encoding="utf-8",
)
PY

expect_rejected duplicate-id "$TMP_DIR/duplicate-id.json"
expect_rejected missing-path "$TMP_DIR/missing-path.json"
expect_rejected l5-without-immutable-evidence "$TMP_DIR/l5-without-immutable-evidence.json"
expect_rejected unknown-gap "$TMP_DIR/unknown-gap.json"
expect_rejected unknown-field "$TMP_DIR/unknown-field.json"
expect_rejected duplicate-key "$TMP_DIR/duplicate-key.json"

for output in "${OUTPUTS[@]}"; do
  run_tool --update --ledger "$LEDGER" >/dev/null
  printf '\n' >> "$output"
  expect_rejected "generated-drift-$(basename "$output")" "$LEDGER"
done
run_tool --update --ledger "$LEDGER" >/dev/null

python3 scripts/lib/roadmap_evidence.py --check >/dev/null
roadmap_identity_count="$(python3 scripts/lib/roadmap_evidence.py --print | wc -l | tr -d ' ')"
[[ "$roadmap_identity_count" == "39" ]] || {
  echo "capability-evidence-ledger-contract: expected 39 roadmap identities, got $roadmap_identity_count" >&2
  exit 1
}
cp ROADMAP.md "$TMP_DIR/roadmap-stale.md"
python3 - "$TMP_DIR/roadmap-stale.md" <<'PY'
from pathlib import Path
import re
import sys

path = Path(sys.argv[1])
text = path.read_text(encoding="utf-8")
text, replacements = re.subn(
    r"capability-ledger-bundle-sha256:[0-9a-f]{64}",
    "capability-ledger-bundle-sha256:" + ("0" * 64),
    text,
    count=1,
)
if replacements != 1:
    raise SystemExit("capability-evidence-ledger-contract: roadmap identity fixture was not mutated")
path.write_text(text, encoding="utf-8")
PY
if python3 scripts/lib/roadmap_evidence.py --check --roadmap "$TMP_DIR/roadmap-stale.md" >/dev/null 2>&1; then
  echo "capability-evidence-ledger-contract: stale roadmap evidence identity was accepted" >&2
  exit 1
fi

echo "capability-evidence-ledger-contract: ok (negative_controls=12 check_mode=read_only generated_views=5 roadmap_identities=$roadmap_identity_count)"
