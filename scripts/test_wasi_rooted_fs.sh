#!/usr/bin/env bash
# Focused actual-target controls, consumed by the existing WASI CI route.
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"
source scripts/lib/cargo_target_dir.sh
RUNTIME="${GENESIS_WASI_ROOTED_RUNTIME:-$(command -v wasmtime || true)}"
if [[ -z "$RUNTIME" || ! -x "$RUNTIME" ]]; then
  echo "test-wasi-rooted-fs: an executable Wasmtime runtime is required" >&2
  exit 1
fi
REPORT="${GENESIS_WASI_ROOTED_REPORT:-$ROOT_DIR/.genesis/perf/wasi-rooted-controls.json}"
mkdir -p "$(dirname "$REPORT")"
EVENTS="${REPORT%.json}.build.jsonl"
genesis_configure_cargo_target_dir "$ROOT_DIR" wasi-rooted-focused-controls root-wasi
trap 'genesis_clear_resolved_cargo_target_dir wasi-rooted-focused-release' EXIT
cargo test -p gc_effects --test wasi_rooted_security --target wasm32-wasip1 \
  --release --locked --offline --no-run --message-format json > "$EVENTS"
BINARY="$(python3 - "$EVENTS" <<'PY'
import json
import sys
from pathlib import Path
rows = [json.loads(line) for line in Path(sys.argv[1]).read_text().splitlines()]
artifacts = [row["executable"] for row in rows if row.get("reason") == "compiler-artifact"
             and row["target"]["name"] == "wasi_rooted_security" and row.get("executable")]
if len(artifacts) != 1:
    raise SystemExit("expected one exact WASI control executable")
print(artifacts[0])
PY
)"
python3 scripts/lib/wasi_rooted_controls.py \
  --binary "$BINARY" --wasmtime "$RUNTIME" --report "$REPORT"
