#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

python3 scripts/lib/release_notes.py --check
python3 scripts/lib/release_notes.py --self-test
echo "release-notes-contract: ok (class=E1 runtime_claims=unverified check_mode=read_only)"
