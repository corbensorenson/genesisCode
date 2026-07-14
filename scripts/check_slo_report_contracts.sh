#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

GAUNTLET_REPORT="${GENESIS_AGENT_GAUNTLET_REPORT:-.genesis/perf/agent_capability_gauntlet_report.json}"
PARITY_REPORT="${GENESIS_AGENT_PARITY_REPORT:-.genesis/perf/agent_workflow_runtime_parity_report.json}"
REQUIRE_PARITY_REPORT="${GENESIS_SLO_REQUIRE_PARITY_REPORT:-0}"

if [[ "$REQUIRE_PARITY_REPORT" != "0" && "$REQUIRE_PARITY_REPORT" != "1" ]]; then
  echo "slo-report-contracts: GENESIS_SLO_REQUIRE_PARITY_REPORT must be 0 or 1" >&2
  exit 2
fi

python3 - "$GAUNTLET_REPORT" "$PARITY_REPORT" "$REQUIRE_PARITY_REPORT" <<'PY'
import json
import pathlib
import sys
from typing import Any

gauntlet_path = pathlib.Path(sys.argv[1])
parity_path = pathlib.Path(sys.argv[2])
require_parity = sys.argv[3] == "1"

if not gauntlet_path.is_file():
    raise SystemExit(f"slo-report-contracts: missing gauntlet report: {gauntlet_path}")
if require_parity and not parity_path.is_file():
    raise SystemExit(f"slo-report-contracts: missing parity report: {parity_path}")

reports: list[tuple[str, pathlib.Path, str]] = [
    ("gauntlet", gauntlet_path, "genesis/agent-capability-gauntlet-v0.1"),
]
if require_parity:
    reports.append(
        ("parity", parity_path, "genesis/agent-workflow-runtime-parity-v0.1")
    )

def require_int(doc: dict[str, Any], key: str, label: str) -> int:
    value = doc.get(key)
    if not isinstance(value, int):
        raise SystemExit(f"slo-report-contracts: {label} missing integer field `{key}`")
    if value <= 0:
        raise SystemExit(f"slo-report-contracts: {label} field `{key}` must be > 0")
    return value

validated = 0
for label, path, expected_kind in reports:
    doc = json.loads(path.read_text(encoding="utf-8"))
    if doc.get("kind") != expected_kind:
        raise SystemExit(
            f"slo-report-contracts: {label} unexpected kind {doc.get('kind')!r} (expected {expected_kind})"
        )

    if not isinstance(doc.get("ok"), bool):
        raise SystemExit(f"slo-report-contracts: {label} missing boolean field `ok`")
    elapsed_ms = require_int(doc, "elapsed_ms", label)
    budget_ms = require_int(doc, "budget_ms", label)
    history_p95_ms = require_int(doc, "history_p95_ms", label)
    history_samples = require_int(doc, "history_samples", label)

    fail_reasons = doc.get("fail_reasons")
    if not isinstance(fail_reasons, list) or not all(
        isinstance(item, str) and item for item in fail_reasons
    ):
        raise SystemExit(
            f"slo-report-contracts: {label} missing string-list field `fail_reasons`"
        )

    if elapsed_ms > budget_ms:
        raise SystemExit(
            f"slo-report-contracts: {label} elapsed budget exceeded ({elapsed_ms} > {budget_ms})"
        )

    p95_min_samples_raw = doc.get("p95_min_samples", 1)
    if not isinstance(p95_min_samples_raw, int) or p95_min_samples_raw < 1:
        raise SystemExit(
            f"slo-report-contracts: {label} field `p95_min_samples` must be an integer >= 1"
        )
    p95_enforced_raw = doc.get("history_p95_enforced")
    if p95_enforced_raw is None:
        p95_enforced = history_samples >= p95_min_samples_raw
    elif isinstance(p95_enforced_raw, bool):
        p95_enforced = p95_enforced_raw
    else:
        raise SystemExit(
            f"slo-report-contracts: {label} field `history_p95_enforced` must be boolean when present"
        )
    if p95_enforced and history_p95_ms > budget_ms:
        raise SystemExit(
            f"slo-report-contracts: {label} history p95 budget exceeded "
            f"({history_p95_ms} > {budget_ms} with {history_samples} samples)"
        )

    ok = bool(doc["ok"])
    if ok and fail_reasons:
        raise SystemExit(
            f"slo-report-contracts: {label} has `ok=true` but non-empty fail_reasons"
        )
    if (not ok) and (not fail_reasons):
        raise SystemExit(
            f"slo-report-contracts: {label} has `ok=false` but empty fail_reasons"
        )
    validated += 1

print(f"slo-report-contracts: ok (validated_reports={validated})")
PY
