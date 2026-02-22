#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

PLAN_FILE="${GENESIS_UPGRADE_PLAN_FILE:-$ROOT_DIR/upgrade_plan.md}"
REPORT_FILE="${GENESIS_REDTEAM_REPORT_FILE:-$ROOT_DIR/docs/status/REDTEAM_REPORT.md}"
READINESS_FILE="${GENESIS_SELFHOST_READINESS_REPORT:-$ROOT_DIR/.genesis/perf/selfhost_readiness_report.json}"
REFRESH_READINESS="${GENESIS_PLANNING_REFRESH_READINESS:-1}"

[[ -f "$PLAN_FILE" ]] || {
  echo "redteam-report: missing plan file at $PLAN_FILE" >&2
  exit 1
}
[[ -f "$REPORT_FILE" ]] || {
  echo "redteam-report: missing report file at $REPORT_FILE" >&2
  exit 1
}
if [[ "$REFRESH_READINESS" != "0" && "$REFRESH_READINESS" != "1" ]]; then
  echo "redteam-report: GENESIS_PLANNING_REFRESH_READINESS must be 0 or 1" >&2
  exit 2
fi
if [[ "$REFRESH_READINESS" == "1" ]]; then
  echo "redteam-report: refreshing readiness report via check_selfhost_readiness_scorecard.sh"
  bash scripts/check_selfhost_readiness_scorecard.sh >/dev/null
elif [[ ! -f "$READINESS_FILE" ]]; then
  echo "redteam-report: readiness report missing; generating via check_selfhost_readiness_scorecard.sh"
  bash scripts/check_selfhost_readiness_scorecard.sh >/dev/null
fi
[[ -f "$READINESS_FILE" ]] || {
  echo "redteam-report: missing readiness report after generation at $READINESS_FILE" >&2
  exit 1
}

python3 - "$PLAN_FILE" "$REPORT_FILE" "$READINESS_FILE" <<'PY'
import json
import pathlib
import re
import sys

plan_path = pathlib.Path(sys.argv[1])
report_path = pathlib.Path(sys.argv[2])
readiness_path = pathlib.Path(sys.argv[3])

plan_text = plan_path.read_text(encoding="utf-8")
report_text = report_path.read_text(encoding="utf-8")
readiness_doc = json.loads(readiness_path.read_text(encoding="utf-8"))

if readiness_doc.get("kind") != "genesis/selfhost-readiness-v0.1":
    raise SystemExit(
        "redteam-report: readiness report kind mismatch (expected genesis/selfhost-readiness-v0.1)"
    )

if not re.search(r"^Last updated:\s+\d{4}-\d{2}-\d{2}$", report_text, re.MULTILINE):
    raise SystemExit(
        "redteam-report: docs/status/REDTEAM_REPORT.md must include 'Last updated: YYYY-MM-DD'"
    )

unresolved_ids = []
for line in plan_text.splitlines():
    m = re.match(r"^- \[ \] (P[01]\.\d+)\b", line)
    if m:
        unresolved_ids.append(m.group(1))
unresolved_ids = sorted(set(unresolved_ids))
readiness_ids = sorted(
    set(
        str(x)
        for x in readiness_doc.get("unresolved_upgrade_plan_ids", [])
        if re.fullmatch(r"P[01]\.\d+", str(x))
    )
)
if unresolved_ids != readiness_ids:
    raise SystemExit(
        "redteam-report: unresolved P0/P1 IDs must match readiness report: "
        f"plan={unresolved_ids} readiness={readiness_ids}"
    )

critical_specs = [
    (
        "agent-capability-gauntlet",
        pathlib.Path(".genesis/perf/agent_capability_gauntlet_report.json"),
        "genesis/agent-capability-gauntlet-v0.1",
    ),
    (
        "production-cli-help-surface",
        pathlib.Path(".genesis/perf/production_cli_help_surface_report.json"),
        "genesis/production-cli-help-surface-v0.1",
    ),
]
critical_failures = []
for label, report_path, expected_kind in critical_specs:
    if not report_path.is_file():
        critical_failures.append(f"{label}:missing")
        continue
    try:
        doc = json.loads(report_path.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        critical_failures.append(f"{label}:json-decode")
        continue
    if doc.get("kind") != expected_kind:
        critical_failures.append(f"{label}:kind-mismatch")
        continue
    if not bool(doc.get("ok", False)):
        critical_failures.append(f"{label}:report-not-ok")

if not unresolved_ids and critical_failures:
    raise SystemExit(
        "redteam-report: unresolved P0/P1 risk set cannot be empty while critical reports are failing: "
        + ", ".join(critical_failures)
    )

report_ids = sorted(set(re.findall(r"\bP[01]\.\d+\b", report_text)))

missing = sorted(set(unresolved_ids) - set(report_ids))
if missing:
    joined = ", ".join(missing)
    raise SystemExit(
        f"redteam-report: unresolved P0/P1 upgrade plan IDs missing from REDTEAM_REPORT.md: {joined}"
    )

extra = sorted(set(report_ids) - set(unresolved_ids))
if extra:
    joined = ", ".join(extra)
    raise SystemExit(
        f"redteam-report: REDTEAM_REPORT.md contains stale P0/P1 IDs not open in upgrade_plan.md: {joined}"
    )

if not unresolved_ids:
    if re.search(r"^No active P0/P1 risks\.$", report_text, re.MULTILINE) is None:
        raise SystemExit(
            "redteam-report: when no unresolved P0/P1 IDs are open, REDTEAM_REPORT.md must include `No active P0/P1 risks.`"
        )

print(
    "redteam-report: ok "
    f"(tracked {len(unresolved_ids)} unresolved P0/P1 IDs in docs/status/REDTEAM_REPORT.md)"
)
PY
