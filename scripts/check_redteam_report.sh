#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if [[ "${GENESIS_PLANNING_REFRESH_READINESS:-0}" == "1" ]]; then
  echo "redteam-report: checks are read-only; refresh runtime readiness with its explicit update command" >&2
  exit 2
fi

# The ledger validator checks exact upgrade-plan ID parity and generated report drift.
bash scripts/check_capability_evidence_ledger.sh >/dev/null

python3 - <<'PY'
import json
from pathlib import Path

ledger = json.loads(
    Path("docs/spec/CAPABILITY_EVIDENCE_LEDGER_v0.1.json").read_text(encoding="utf-8")
)
print(
    "redteam-report: ok "
    f"(active_p0_p1={len(ledger['active_defect_ids'])} read_only=true)"
)
PY
