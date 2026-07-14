#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

retained_paths() {
  printf '%s\n' \
    genesis.compatibility.json \
    docs/spec/V1_COMPATIBILITY_REGISTRY_v0.1.schema.json \
    docs/spec/VERSION_SURFACES_v0.1.md \
    scripts/lib/v1_compatibility.py
  python3 - <<'PY'
import json
from pathlib import Path

data = json.loads(Path("genesis.compatibility.json").read_text(encoding="utf-8"))
for entry in data["entries"]:
    for authority in entry["authorities"]:
        print(authority["path"])
PY
}

retained_digest() {
  retained_paths | LC_ALL=C sort -u | while IFS= read -r path; do
    cksum "$path"
  done
}

before="$(retained_digest)"
python3 scripts/lib/v1_compatibility.py "$ROOT_DIR"
python3 scripts/lib/v1_compatibility.py "$ROOT_DIR" --self-test
after="$(retained_digest)"

if [[ "$before" != "$after" ]]; then
  echo "v1-compatibility: read-only check mutated a retained input" >&2
  exit 1
fi

echo "v1-compatibility-contract: ok (check_mode=read_only duplicate_keys=rejected)"
