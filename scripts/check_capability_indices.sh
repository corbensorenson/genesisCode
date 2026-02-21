#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

HOST_INDEX="docs/spec/HOST_ABI_INDEX_v0.1.json"
HOST_SCHEMA_INDEX="docs/spec/HOST_ABI_SCHEMA_INDEX_v0.1.json"
PRELUDE_INDEX="docs/spec/PRELUDE_CAPABILITY_INDEX_v0.1.json"

for f in "$HOST_INDEX" "$HOST_SCHEMA_INDEX" "$PRELUDE_INDEX"; do
  if [[ ! -f "$f" ]]; then
    echo "capability-indices: missing index file: $f" >&2
    echo "capability-indices: run scripts/update_capability_indices.sh" >&2
    exit 1
  fi
done

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

python3 scripts/generate_capability_indices.py \
  --root "$ROOT_DIR" \
  --out-host "$TMP_DIR/HOST_ABI_INDEX_v0.1.json" \
  --out-host-schema "$TMP_DIR/HOST_ABI_SCHEMA_INDEX_v0.1.json" \
  --out-prelude "$TMP_DIR/PRELUDE_CAPABILITY_INDEX_v0.1.json"

if ! cmp -s "$TMP_DIR/HOST_ABI_INDEX_v0.1.json" "$HOST_INDEX"; then
  echo "capability-indices: host ABI index is stale: $HOST_INDEX" >&2
  diff -u "$HOST_INDEX" "$TMP_DIR/HOST_ABI_INDEX_v0.1.json" || true
  echo "capability-indices: run scripts/update_capability_indices.sh" >&2
  exit 1
fi

if ! cmp -s "$TMP_DIR/HOST_ABI_SCHEMA_INDEX_v0.1.json" "$HOST_SCHEMA_INDEX"; then
  echo "capability-indices: host ABI schema index is stale: $HOST_SCHEMA_INDEX" >&2
  diff -u "$HOST_SCHEMA_INDEX" "$TMP_DIR/HOST_ABI_SCHEMA_INDEX_v0.1.json" || true
  echo "capability-indices: run scripts/update_capability_indices.sh" >&2
  exit 1
fi

if ! cmp -s "$TMP_DIR/PRELUDE_CAPABILITY_INDEX_v0.1.json" "$PRELUDE_INDEX"; then
  echo "capability-indices: prelude capability index is stale: $PRELUDE_INDEX" >&2
  diff -u "$PRELUDE_INDEX" "$TMP_DIR/PRELUDE_CAPABILITY_INDEX_v0.1.json" || true
  echo "capability-indices: run scripts/update_capability_indices.sh" >&2
  exit 1
fi

echo "capability-indices: ok"
