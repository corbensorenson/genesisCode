#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

retained_digest() {
  for path in \
    genesis.gates.json \
    genesis.prerequisites.json \
    policies/gates_v0.1.json \
    docs/spec/GATE_MANIFEST_v0.1.schema.json \
    docs/spec/CHECK_UPDATE_BOUNDARY_AUDIT_v0.1.json \
    scripts/lib/gate_manifest.py
  do
    cksum "$path"
  done
}

before="$(retained_digest)"
python3 scripts/lib/gate_manifest.py --check --self-test
after="$(retained_digest)"

if [[ "$before" != "$after" ]]; then
  echo "gate-manifest: read-only check mutated a retained input" >&2
  exit 1
fi

echo "gate-manifest-contract: ok (check_mode=read_only duplicate_keys=rejected)"
