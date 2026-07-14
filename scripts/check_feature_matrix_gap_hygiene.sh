#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if [[ "${GENESIS_PLANNING_REFRESH_READINESS:-0}" == "1" ]]; then
  echo "feature-matrix-gap-hygiene: checks are read-only; refresh readiness with its explicit update command" >&2
  exit 2
fi

# The canonical checker validates roadmap gap IDs, exact upgrade-plan defect IDs,
# and generated feature-matrix drift without building or writing reports.
bash scripts/check_capability_evidence_ledger.sh >/dev/null

python3 - <<'PY'
import json
import pathlib
import re

root = pathlib.Path.cwd()
ledger_path = root / "docs/spec/CAPABILITY_EVIDENCE_LEDGER_v0.1.json"
plan_path = root / "upgrade_plan.md"

ledger = json.loads(ledger_path.read_text(encoding="utf-8"))
ledger_ids = sorted(set(str(item) for item in ledger.get("active_defect_ids", [])))
plan_ids = []
for line in plan_path.read_text(encoding="utf-8").splitlines():
    match = re.match(r"^- \[ \] (P\d+\.\d+)\b", line)
    if match:
        plan_ids.append(match.group(1))
plan_ids = sorted(set(plan_ids))
if ledger_ids != plan_ids:
    raise SystemExit(
        "feature-matrix-gap-hygiene: ledger active defects must match upgrade plan: "
        f"ledger={ledger_ids} plan={plan_ids}"
    )

roadmap_gap_count = len(
    {
        gap
        for claim in ledger.get("claims", [])
        for gap in claim.get("gap_ids", [])
        if str(gap).startswith(("R", "F"))
    }
)
print(
    "feature-matrix-gap-hygiene: ok "
    f"(active_defects={len(plan_ids)} roadmap_gaps={roadmap_gap_count} read_only=true)"
)
PY
