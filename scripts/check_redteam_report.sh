#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

PLAN_FILE="${GENESIS_UPGRADE_PLAN_FILE:-$ROOT_DIR/upgrade_plan.md}"
REPORT_FILE="${GENESIS_REDTEAM_REPORT_FILE:-$ROOT_DIR/docs/status/REDTEAM_REPORT.md}"

[[ -f "$PLAN_FILE" ]] || {
  echo "redteam-report: missing plan file at $PLAN_FILE" >&2
  exit 1
}
[[ -f "$REPORT_FILE" ]] || {
  echo "redteam-report: missing report file at $REPORT_FILE" >&2
  exit 1
}

python3 - "$PLAN_FILE" "$REPORT_FILE" <<'PY'
import pathlib
import re
import sys

plan_path = pathlib.Path(sys.argv[1])
report_path = pathlib.Path(sys.argv[2])

plan_text = plan_path.read_text(encoding="utf-8")
report_text = report_path.read_text(encoding="utf-8")

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
