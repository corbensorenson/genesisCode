#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/genesis-evidence-storage.XXXXXX")"
trap 'rm -rf "$TMP_DIR"' EXIT

POLICY="policies/evidence_storage_classes_v0.1.json"
CATALOG="docs/program/EVIDENCE_FIXTURE_CLASSIFICATION_v0.1.json"

snapshot_retained() {
  python3 - <<'PY'
from hashlib import sha256
from pathlib import Path
import json

paths = [
    Path("policies/evidence_storage_classes_v0.1.json"),
    Path("docs/program/EVIDENCE_FIXTURE_CLASSIFICATION_v0.1.json"),
    *sorted(path for path in Path("docs/program/evidence").rglob("*") if path.is_file()),
]
print(json.dumps({path.as_posix(): sha256(path.read_bytes()).hexdigest() for path in paths}, sort_keys=True))
PY
}

expect_fail() {
  local label="$1"
  shift
  if "$@" >"$TMP_DIR/$label.stdout" 2>"$TMP_DIR/$label.stderr"; then
    echo "evidence-storage-classes: negative control was accepted: $label" >&2
    exit 1
  fi
}

before="$(snapshot_retained)"

python3 scripts/lib/evidence_storage.py check-policy
python3 scripts/lib/evidence_storage.py \
  render-fixture-catalog \
  --output "$TMP_DIR/fixture-catalog.json"
cmp -s "$CATALOG" "$TMP_DIR/fixture-catalog.json" || {
  echo "evidence-storage-classes: fixture catalog drift" >&2
  echo "evidence-storage-classes: run bash scripts/update_evidence_fixture_classification.sh" >&2
  exit 1
}

for probe in \
  .genesis/e0-observation.json \
  .genesis/release-assets/evidence/E3/probe \
  .genesis/release-assets/evidence/E4/probe; do
  git check-ignore -q "$probe" || {
    echo "evidence-storage-classes: required ignored storage path is not ignored: $probe" >&2
    exit 1
  }
done

if [[ -n "$(git ls-files '.genesis/**')" ]]; then
  echo "evidence-storage-classes: E0/E3/E4 local storage must not be tracked" >&2
  git ls-files '.genesis/**' >&2
  exit 1
fi

bash scripts/render_evidence_release_asset.sh \
  "$TMP_DIR/release-a" E3 conformance-v0.1 >/dev/null
bash scripts/render_evidence_release_asset.sh \
  "$TMP_DIR/release-b" E3 conformance-v0.1 >/dev/null

python3 - "$TMP_DIR/release-a" "$TMP_DIR/release-b" <<'PY'
from hashlib import sha256
from pathlib import Path
import sys

def snapshot(root: Path):
    return {
        path.relative_to(root).as_posix(): sha256(path.read_bytes()).hexdigest()
        for path in root.rglob("*")
        if path.is_file()
    }

left = snapshot(Path(sys.argv[1]))
right = snapshot(Path(sys.argv[2]))
if left != right:
    raise SystemExit(f"evidence-storage-classes: release renders differ: {left!r} != {right!r}")
PY

python3 scripts/lib/evidence_storage.py \
  verify-release --release-dir "$TMP_DIR/release-a" >/dev/null
python3 scripts/lib/evidence_storage.py \
  mirror-release \
  --source-dir "$TMP_DIR/release-a" \
  --destination-dir "$TMP_DIR/mirror" >/dev/null

expect_fail immutable-rerender \
  bash scripts/render_evidence_release_asset.sh \
    "$TMP_DIR/release-a" E3 conformance-v0.1
expect_fail mirror-overwrite \
  python3 scripts/lib/evidence_storage.py mirror-release \
    --source-dir "$TMP_DIR/release-a" \
    --destination-dir "$TMP_DIR/mirror"
expect_fail class-escalation \
  bash scripts/render_evidence_release_asset.sh \
    "$TMP_DIR/e4-rejected" E4 conformance-v0.1
expect_fail non-release-class \
  bash scripts/render_evidence_release_asset.sh \
    "$TMP_DIR/e2-rejected" E2 conformance-v0.1

cp -R "$TMP_DIR/release-a" "$TMP_DIR/tampered"
tampered_asset="$(find "$TMP_DIR/tampered" -maxdepth 1 -type f -name '*.tar' -print -quit)"
printf 'x' >>"$tampered_asset"
expect_fail archive-tamper \
  python3 scripts/lib/evidence_storage.py \
    verify-release --release-dir "$TMP_DIR/tampered"

cp "$POLICY" "$TMP_DIR/duplicate-policy.json"
python3 - "$TMP_DIR/duplicate-policy.json" <<'PY'
from pathlib import Path
import sys

path = Path(sys.argv[1])
text = path.read_text(encoding="utf-8")
needle = '  "version": "0.1",\n'
if text.count(needle) != 1:
    raise SystemExit("evidence-storage-classes: policy duplicate-key anchor drift")
path.write_text(text.replace(needle, '  "version": "0.1",\n  "version": "0.1",\n', 1), encoding="utf-8")
PY
expect_fail duplicate-policy-key \
  python3 scripts/lib/evidence_storage.py \
    --policy "$TMP_DIR/duplicate-policy.json" check-policy

cp "$CATALOG" "$TMP_DIR/escalated-catalog.json"
python3 - "$TMP_DIR/escalated-catalog.json" <<'PY'
from pathlib import Path
import sys

path = Path(sys.argv[1])
text = path.read_text(encoding="utf-8")
needle = '  "distributionClass": "E2",\n'
if text.count(needle) != 1:
    raise SystemExit("evidence-storage-classes: catalog escalation anchor drift")
path.write_text(text.replace(needle, '  "distributionClass": "E3",\n', 1), encoding="utf-8")
PY
expect_fail fixture-authority-escalation \
  python3 scripts/lib/evidence_storage.py \
    --fixture-catalog "$TMP_DIR/escalated-catalog.json" check-policy

python3 - "$TMP_DIR/release-a" "$TMP_DIR/traversal" <<'PY'
from hashlib import sha256
import io
import json
from pathlib import Path
import sys
import tarfile

source = Path(sys.argv[1])
dest = Path(sys.argv[2])
dest.mkdir()
old_asset = next(source.glob("*.tar"))
members = []
with tarfile.open(old_asset, "r:") as archive:
    for member in archive.getmembers():
        stream = archive.extractfile(member)
        if stream is None:
            raise SystemExit("evidence-storage-classes: traversal fixture source member unreadable")
        members.append((member.name, stream.read()))
members.append(("../escape", b"forbidden\n"))
members.sort()
buffer = io.BytesIO()
with tarfile.open(fileobj=buffer, mode="w", format=tarfile.USTAR_FORMAT) as archive:
    for name, data in members:
        info = tarfile.TarInfo(name)
        info.size = len(data)
        info.mtime = 0
        info.mode = 0o644
        info.uid = 0
        info.gid = 0
        info.uname = ""
        info.gname = ""
        archive.addfile(info, io.BytesIO(data))
payload = buffer.getvalue()
digest = sha256(payload).hexdigest()
prefix = old_asset.name.split("sha256-", 1)[0]
asset_name = f"{prefix}sha256-{digest}.tar"
(dest / asset_name).write_bytes(payload)
(dest / f"{asset_name}.sha256").write_text(f"{digest}  {asset_name}\n", encoding="ascii")
descriptor_path = source / f"{old_asset.name}.mirror.json"
descriptor = json.loads(descriptor_path.read_text(encoding="utf-8"))
descriptor["asset"]["name"] = asset_name
descriptor["asset"]["sha256"] = digest
descriptor["asset"]["sizeBytes"] = len(payload)
for instruction in descriptor["instructions"]:
    instruction["argv"] = [asset_name if item == old_asset.name else item for item in instruction["argv"]]
    if instruction["id"] == "verify-sha256":
        instruction["argv"][-1] = f"{digest}  {asset_name}"
(dest / f"{asset_name}.mirror.json").write_text(
    json.dumps(descriptor, sort_keys=True, indent=2) + "\n", encoding="utf-8"
)
PY
expect_fail archive-path-traversal \
  python3 scripts/lib/evidence_storage.py \
    verify-release --release-dir "$TMP_DIR/traversal"

after="$(snapshot_retained)"
[[ "$before" == "$after" ]] || {
  echo "evidence-storage-classes: check mutated retained evidence" >&2
  exit 1
}

fixture_count="$(python3 - <<'PY'
import json
print(len(json.load(open("docs/program/EVIDENCE_FIXTURE_CLASSIFICATION_v0.1.json", encoding="utf-8"))["files"]))
PY
)"
echo "evidence-storage-classes-contract: ok (classes=5 fixtures=$fixture_count release=E3 mirrors=1 negative_controls=8 check_mode=read_only)"
