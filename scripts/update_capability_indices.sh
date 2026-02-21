#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

python3 scripts/generate_capability_indices.py \
  --root "$ROOT_DIR" \
  --out-host "$ROOT_DIR/docs/spec/HOST_ABI_INDEX_v0.1.json" \
  --out-host-schema "$ROOT_DIR/docs/spec/HOST_ABI_SCHEMA_INDEX_v0.1.json" \
  --out-prelude "$ROOT_DIR/docs/spec/PRELUDE_CAPABILITY_INDEX_v0.1.json"

echo "capability-indices: wrote docs/spec/HOST_ABI_INDEX_v0.1.json"
echo "capability-indices: wrote docs/spec/HOST_ABI_SCHEMA_INDEX_v0.1.json"
echo "capability-indices: wrote docs/spec/PRELUDE_CAPABILITY_INDEX_v0.1.json"
