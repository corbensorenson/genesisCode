#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

MANIFEST_FILE="$ROOT_DIR/selfhost/toolchain_manifest.gc"
ARTIFACT_FILE="$ROOT_DIR/selfhost/toolchain.gc"
OUT_FILE="$ROOT_DIR/selfhost/toolchain.freshness.json"

[[ -f "$MANIFEST_FILE" ]] || {
  echo "selfhost-freshness: missing manifest: $MANIFEST_FILE" >&2
  exit 1
}
[[ -f "$ARTIFACT_FILE" ]] || {
  echo "selfhost-freshness: missing artifact: $ARTIFACT_FILE" >&2
  exit 1
}

python3 - "$ROOT_DIR" "$MANIFEST_FILE" "$ARTIFACT_FILE" "$OUT_FILE" <<'PY'
import hashlib
import json
import pathlib
import re
import sys

root = pathlib.Path(sys.argv[1]).resolve()
manifest = pathlib.Path(sys.argv[2]).resolve()
artifact = pathlib.Path(sys.argv[3]).resolve()
out = pathlib.Path(sys.argv[4]).resolve()

text = manifest.read_text(encoding="utf-8")
paths = sorted(set(re.findall(r'"(selfhost/[A-Za-z0-9_./-]+\.gc)"', text)))
if not paths:
    raise SystemExit("selfhost-freshness: no module paths found in selfhost/toolchain_manifest.gc")

h = hashlib.sha256()
h.update(b"manifest\0")
h.update(str(manifest.relative_to(root)).encode("utf-8"))
h.update(b"\0")
h.update(manifest.read_bytes())
for rel in paths:
    p = root / rel
    if not p.is_file():
        raise SystemExit(f"selfhost-freshness: missing module listed in manifest: {rel}")
    h.update(b"\0module\0")
    h.update(rel.encode("utf-8"))
    h.update(b"\0")
    h.update(p.read_bytes())

artifact_hash = hashlib.sha256(artifact.read_bytes()).hexdigest()
payload = {
    "kind": "genesis/selfhost-freshness-v0.1",
    "manifest": str(manifest.relative_to(root)),
    "artifact": str(artifact.relative_to(root)),
    "source_hash_sha256": h.hexdigest(),
    "artifact_hash_sha256": artifact_hash,
}
out.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(out)
PY

echo "selfhost-freshness: updated $OUT_FILE"
