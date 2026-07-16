#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODULES_DIR="$ROOT_DIR/prelude/modules"
MANIFEST_FILE="$MODULES_DIR/manifest.toml"
OUT_FILE="${GENESIS_PRELUDE_OUT:-$ROOT_DIR/prelude/prelude.gc}"
MANIFEST_HASH_FILE="${GENESIS_PRELUDE_MANIFEST_HASH_OUT:-$ROOT_DIR/prelude/prelude.manifest.sha256}"

if [ ! -d "$MODULES_DIR" ]; then
  echo "modules directory missing: $MODULES_DIR" >&2
  exit 1
fi
if [ ! -f "$MANIFEST_FILE" ]; then
  echo "prelude manifest missing: $MANIFEST_FILE" >&2
  exit 1
fi

python3 - "$MODULES_DIR" "$MANIFEST_FILE" "$OUT_FILE" "$MANIFEST_HASH_FILE" <<'PY'
import hashlib
import json
import pathlib
import sys
sys.path.insert(0, str(pathlib.Path(sys.argv[1]).resolve().parents[1] / "scripts/lib"))
from toml_compat import tomllib

modules_dir = pathlib.Path(sys.argv[1])
manifest_path = pathlib.Path(sys.argv[2])
out_path = pathlib.Path(sys.argv[3])
manifest_hash_path = pathlib.Path(sys.argv[4])

manifest = tomllib.loads(manifest_path.read_text(encoding="utf-8"))
if manifest.get("version") != 1:
    raise SystemExit(f"prelude manifest: expected version=1 in {manifest_path}")

modules = manifest.get("modules")
if not isinstance(modules, list) or not modules:
    raise SystemExit("prelude manifest: `modules` must be a non-empty array")
if len(modules) != len(set(modules)):
    raise SystemExit("prelude manifest: duplicate module entries are not allowed")

deps = manifest.get("deps", {})
if deps is None:
    deps = {}
if not isinstance(deps, dict):
    raise SystemExit("prelude manifest: `deps` must be a table")

module_set = set(modules)
ordered_paths = []
for m in modules:
    if not isinstance(m, str) or not m.endswith(".gc"):
        raise SystemExit(f"prelude manifest: invalid module entry `{m}`")
    p = modules_dir / m
    if not p.is_file():
        raise SystemExit(f"prelude manifest: listed module missing: {p}")
    ordered_paths.append(p)

actual_gc_files = {p.name for p in modules_dir.glob("*.gc")}
unexpected = sorted(actual_gc_files - module_set)
if unexpected:
    raise SystemExit(
        "prelude manifest: unlisted module files present: "
        + ", ".join(unexpected)
    )

module_index = {name: i for i, name in enumerate(modules)}
for module_name, dep_list in deps.items():
    if module_name not in module_set:
        raise SystemExit(
            f"prelude manifest: deps entry references unknown module `{module_name}`"
        )
    if not isinstance(dep_list, list):
        raise SystemExit(
            f"prelude manifest: deps for `{module_name}` must be an array"
        )
    for dep in dep_list:
        if dep not in module_set:
            raise SystemExit(
                f"prelude manifest: dependency `{dep}` for `{module_name}` is not listed in modules"
            )
        if module_index[dep] >= module_index[module_name]:
            raise SystemExit(
                f"prelude manifest: dependency order violation `{module_name}` depends on `{dep}` but appears before it"
            )

assembled_chunks = []
for p in ordered_paths:
    assembled_chunks.append(p.read_text(encoding="utf-8"))
assembled = "\n".join(assembled_chunks) + "\n"
out_path.write_text(assembled, encoding="utf-8")

manifest_canonical = json.dumps(manifest, sort_keys=True, separators=(",", ":"))
manifest_hash = hashlib.sha256(manifest_canonical.encode("utf-8")).hexdigest()
manifest_hash_path.write_text(
    json.dumps(
        {
            "kind": "genesis/prelude-manifest-hash-v0.1",
            "manifest": str(manifest_path.relative_to(modules_dir.parent.parent)),
            "sha256": manifest_hash,
        },
        indent=2,
        sort_keys=True,
    )
    + "\n",
    encoding="utf-8",
)
PY
