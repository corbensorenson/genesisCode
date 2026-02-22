#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

ARTIFACT_PATH="${GENESIS_SELFHOST_TOOLCHAIN_ARTIFACT:-selfhost/toolchain.gc}"
MANIFEST_PATH="${GENESIS_SELFHOST_TOOLCHAIN_MANIFEST:-selfhost/toolchain_manifest.gc}"
FRESHNESS_PATH="${GENESIS_SELFHOST_TOOLCHAIN_FRESHNESS:-selfhost/toolchain.freshness.json}"
OUT_PATH="${1:-selfhost/toolchain.review.md}"

python3 - "$ROOT_DIR" "$ARTIFACT_PATH" "$MANIFEST_PATH" "$FRESHNESS_PATH" "$OUT_PATH" <<'PY'
import hashlib
import json
import pathlib
import re
import sys

root = pathlib.Path(sys.argv[1])
artifact = root / sys.argv[2]
manifest = root / sys.argv[3]
freshness = root / sys.argv[4]
out_path = root / sys.argv[5]

def display_path(path: pathlib.Path) -> str:
    try:
        return path.relative_to(root).as_posix()
    except ValueError:
        return str(path)

for path, label in (
    (artifact, "artifact"),
    (manifest, "manifest"),
    (freshness, "freshness"),
):
    if not path.is_file():
        raise SystemExit(f"update-selfhost-toolchain-review: missing {label} file: {path}")

fresh = json.loads(freshness.read_text(encoding="utf-8"))
if fresh.get("kind") != "genesis/selfhost-freshness-v0.1":
    raise SystemExit(
        "update-selfhost-toolchain-review: unsupported freshness kind in selfhost/toolchain.freshness.json"
    )

manifest_text = manifest.read_text(encoding="utf-8")
module_paths = re.findall(r'"(selfhost/[^"]+\.gc)"', manifest_text)
if not module_paths:
    raise SystemExit(
        "update-selfhost-toolchain-review: no module paths discovered in selfhost/toolchain_manifest.gc"
    )

rows = []
for rel in module_paths:
    path = root / rel
    if not path.is_file():
        raise SystemExit(
            f"update-selfhost-toolchain-review: manifest path missing from workspace: {rel}"
        )
    src = path.read_text(encoding="utf-8")
    defs = re.findall(r"\(def\s+([^\s\)]+)", src)
    data = path.read_bytes()
    rows.append(
        {
            "path": rel,
            "lines": len(src.splitlines()),
            "bytes": len(data),
            "sha256": hashlib.sha256(data).hexdigest(),
            "def_count": len(defs),
            "first_defs": defs[:8],
        }
    )

aggregate_source_hash = hashlib.sha256(
    "".join(f"{r['path']}:{r['sha256']}\n" for r in rows).encode("utf-8")
).hexdigest()
artifact_sha256 = hashlib.sha256(artifact.read_bytes()).hexdigest()

lines = [
    "# Selfhost Toolchain Review Sidecar (v0.1)",
    "",
    "Deterministic review-sidecar for `selfhost/toolchain.gc`.",
    "",
    "## Artifact Identity",
    "",
    f"- Artifact path: `{artifact.relative_to(root).as_posix()}`",
    f"- Artifact sha256: `{artifact_sha256}`",
    f"- Freshness artifact hash: `{fresh.get('artifact_hash_sha256', 'unknown')}`",
    f"- Freshness source hash: `{fresh.get('source_hash_sha256', 'unknown')}`",
    f"- Source aggregate hash (module path + module sha256): `{aggregate_source_hash}`",
    f"- Manifest path: `{manifest.relative_to(root).as_posix()}`",
    f"- Module count: `{len(rows)}`",
    "",
    "## Module Summary",
    "",
    "| Module | Lines | Bytes | Defs | SHA256 |",
    "| --- | ---: | ---: | ---: | --- |",
]

for row in rows:
    lines.append(
        f"| `{row['path']}` | {row['lines']} | {row['bytes']} | {row['def_count']} | `{row['sha256'][:16]}` |"
    )

lines.extend(["", "## Export Surface (Preview)", ""])
for row in rows:
    preview = ", ".join(f"`{name}`" for name in row["first_defs"]) or "_none_"
    lines.append(f"- `{row['path']}`: {preview}")

out_path.parent.mkdir(parents=True, exist_ok=True)
out_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
print(
    "update-selfhost-toolchain-review: wrote "
    f"{display_path(out_path)} "
    f"(modules={len(rows)} aggregate={aggregate_source_hash[:16]})"
)
PY
