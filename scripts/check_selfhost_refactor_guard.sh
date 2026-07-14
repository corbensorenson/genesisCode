#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

MANIFEST="$ROOT_DIR/selfhost/toolchain_manifest.gc"
MIN_MODULES="${GENESIS_SELFHOST_MIN_MODULE_COUNT:-12}"

[[ "$MIN_MODULES" =~ ^[0-9]+$ ]] || {
  echo "selfhost-refactor-guard: GENESIS_SELFHOST_MIN_MODULE_COUNT must be numeric" >&2
  exit 1
}

python3 - "$MANIFEST" "$MIN_MODULES" <<'PY'
import pathlib
import re
import sys

manifest = pathlib.Path(sys.argv[1]).resolve()
min_modules = int(sys.argv[2])
text = manifest.read_text(encoding="utf-8")

m = re.search(r":module-paths\s*\[(?P<body>.*?)\]", text, flags=re.S)
if not m:
    raise SystemExit("selfhost-refactor-guard: manifest missing :module-paths vector")
paths = re.findall(r'"(selfhost/[A-Za-z0-9_./-]+\.gc)"', m.group("body"))
if not paths:
    raise SystemExit("selfhost-refactor-guard: manifest has zero module paths")
if len(paths) != len(set(paths)):
    raise SystemExit("selfhost-refactor-guard: manifest has duplicate module paths")
if len(paths) < min_modules:
    raise SystemExit(
        f"selfhost-refactor-guard: module-path count {len(paths)} below minimum {min_modules}"
    )
if "selfhost/toolchain.gc" in paths:
    raise SystemExit("selfhost-refactor-guard: toolchain artifact must not be listed as module source")

root = manifest.parents[1]
for rel in paths:
    p = root / rel
    if not p.is_file():
        raise SystemExit(f"selfhost-refactor-guard: manifest module missing: {rel}")

print(f"selfhost-refactor-guard: manifest module-path count={len(paths)} min={min_modules}")
PY

python3 scripts/selfhost_refactor_pipeline.py --repo-root "$ROOT_DIR" verify

echo "selfhost-refactor-guard: ok"
