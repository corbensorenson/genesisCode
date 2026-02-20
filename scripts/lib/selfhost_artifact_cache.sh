#!/usr/bin/env bash
set -euo pipefail

selfhost_source_hash() {
  local root="$1"
  local manifest="$root/selfhost/toolchain_manifest.gc"
  python3 - "$root" "$manifest" <<'PY'
import hashlib
import pathlib
import re
import sys

root = pathlib.Path(sys.argv[1]).resolve()
manifest = pathlib.Path(sys.argv[2]).resolve()
text = manifest.read_text(encoding="utf-8")
paths = sorted(set(re.findall(r'"(selfhost/[A-Za-z0-9_./-]+\.gc)"', text)))
if not paths:
    raise SystemExit("selfhost-artifact-cache: no module paths found in selfhost/toolchain_manifest.gc")

h = hashlib.sha256()
h.update(b"manifest\0")
h.update(str(manifest.relative_to(root)).encode("utf-8"))
h.update(b"\0")
h.update(manifest.read_bytes())

for rel in paths:
    p = root / rel
    if not p.is_file():
        raise SystemExit(f"selfhost-artifact-cache: missing module listed in manifest: {rel}")
    h.update(b"\0module\0")
    h.update(rel.encode("utf-8"))
    h.update(b"\0")
    h.update(p.read_bytes())

print(h.hexdigest())
PY
}

selfhost_repo_artifact_matches_source_hash() {
  local root="$1"
  local source_hash="$2"
  local freshness="$root/selfhost/toolchain.freshness.json"
  local artifact="$root/selfhost/toolchain.gc"
  [[ -f "$freshness" ]] || return 1
  [[ -f "$artifact" ]] || return 1
  python3 - "$freshness" "$artifact" "$source_hash" <<'PY'
import hashlib
import json
import pathlib
import sys

freshness = pathlib.Path(sys.argv[1])
artifact = pathlib.Path(sys.argv[2])
expected_source_hash = sys.argv[3]
try:
    meta = json.loads(freshness.read_text(encoding="utf-8"))
except Exception:
    raise SystemExit(1)

actual_artifact_hash = hashlib.sha256(artifact.read_bytes()).hexdigest()
ok = (
    isinstance(meta, dict)
    and meta.get("kind") == "genesis/selfhost-freshness-v0.1"
    and meta.get("source_hash_sha256") == expected_source_hash
    and meta.get("artifact_hash_sha256") == actual_artifact_hash
)
if not ok:
    raise SystemExit(1)
PY
}

resolve_cached_selfhost_artifact() {
  local root="$1"
  local genesis_bin="$2"
  local source_hash
  source_hash="$(selfhost_source_hash "$root")"
  local cache_dir="${GENESIS_SELFHOST_ARTIFACT_CACHE_DIR:-$root/.genesis/cache/selfhost_toolchain}"
  local cache_artifact="$cache_dir/${source_hash}.gc"
  local force_rebuild="${GENESIS_REBUILD_SELFHOST_ARTIFACT:-0}"
  mkdir -p "$cache_dir"

  if [[ "$force_rebuild" != "1" && -f "$cache_artifact" ]]; then
    echo "$cache_artifact"
    return 0
  fi

  local repo_artifact="$root/selfhost/toolchain.gc"
  if [[ "$force_rebuild" != "1" ]] && selfhost_repo_artifact_matches_source_hash "$root" "$source_hash"; then
    cp "$repo_artifact" "$cache_artifact"
    echo "$cache_artifact"
    return 0
  fi

  "$genesis_bin" selfhost-artifact --out "$cache_artifact" >/dev/null
  echo "$cache_artifact"
}
