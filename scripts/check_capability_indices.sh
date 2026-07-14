#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

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

report_stale_index() {
  local label="$1"
  local tracked="$2"
  local generated="$3"

  echo "capability-indices: $label is stale: $tracked" >&2
  if [[ "${GITHUB_ACTIONS:-}" == "true" ]]; then
    echo "::error file=${tracked},title=Stale capability index::${tracked} is stale; run scripts/update_capability_indices.sh"
    echo "::group::capability-indices diff: ${tracked}"
  fi
  diff -u "$tracked" "$generated" || true
  if [[ "${GITHUB_ACTIONS:-}" == "true" ]]; then
    echo "::endgroup::"
  fi
  echo "capability-indices: run scripts/update_capability_indices.sh" >&2
}

if ! cmp -s "$TMP_DIR/HOST_ABI_INDEX_v0.1.json" "$HOST_INDEX"; then
  report_stale_index "host ABI index" "$HOST_INDEX" "$TMP_DIR/HOST_ABI_INDEX_v0.1.json"
  exit 1
fi

if ! cmp -s "$TMP_DIR/HOST_ABI_SCHEMA_INDEX_v0.1.json" "$HOST_SCHEMA_INDEX"; then
  report_stale_index "host ABI schema index" "$HOST_SCHEMA_INDEX" "$TMP_DIR/HOST_ABI_SCHEMA_INDEX_v0.1.json"
  exit 1
fi

if ! cmp -s "$TMP_DIR/PRELUDE_CAPABILITY_INDEX_v0.1.json" "$PRELUDE_INDEX"; then
  report_stale_index "prelude capability index" "$PRELUDE_INDEX" "$TMP_DIR/PRELUDE_CAPABILITY_INDEX_v0.1.json"
  exit 1
fi

echo "capability-indices: ok"
