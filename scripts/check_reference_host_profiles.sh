#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/genesis-reference-hosts.XXXXXX")"
trap 'rm -rf "$TMP_DIR"' EXIT

INPUTS=(
  policies/reference_host_profiles_v0.1.json
  docs/spec/REFERENCE_HOST_PROFILES_v0.1.schema.json
  docs/spec/REFERENCE_HOST_OBSERVATION_v0.1.schema.json
  docs/spec/CHECK_UPDATE_BOUNDARY_v0.1.md
  genesis.prerequisites.json
  scripts/lib/reference_host_profiles.py
  scripts/render_reference_host_observation.sh
)

snapshot() {
  python3 - "${INPUTS[@]}" <<'PY'
from hashlib import sha256
from pathlib import Path
import sys

for raw in sys.argv[1:]:
    path = Path(raw)
    print(f"{sha256(path.read_bytes()).hexdigest()}  {path.as_posix()}")
PY
}

before="$(snapshot)"
python3 scripts/lib/reference_host_profiles.py check
python3 scripts/lib/reference_host_profiles.py self-test
bash scripts/render_reference_host_observation.sh "$TMP_DIR/observation-a.json"
bash scripts/render_reference_host_observation.sh "$TMP_DIR/observation-b.json"
cmp -s "$TMP_DIR/observation-a.json" "$TMP_DIR/observation-b.json" || {
  echo "reference-host-profiles: repeated host observations differ" >&2
  exit 1
}
python3 scripts/lib/reference_host_profiles.py verify-observation \
  --observation "$TMP_DIR/observation-a.json"
if rg -n '/Users/|/home/|[A-Za-z]:\\Users\\|hostname|userName|serialNumber' \
  "$TMP_DIR/observation-a.json"; then
  echo "reference-host-profiles: observation leaked forbidden host identity material" >&2
  exit 1
fi
after="$(snapshot)"
[[ "$before" == "$after" ]] || {
  echo "reference-host-profiles: check mutated retained inputs" >&2
  exit 1
}

echo "reference-host-profiles-contract: ok (profiles=4 tier1=2 tier2=2 dimensions=6 controls=9 check_mode=read_only)"
