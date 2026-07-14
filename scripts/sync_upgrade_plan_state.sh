#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

PLAN_FILE="${GENESIS_UPGRADE_PLAN_FILE:-$ROOT_DIR/upgrade_plan.md}"
REPORT_FILE="${GENESIS_REDTEAM_REPORT_FILE:-$ROOT_DIR/docs/status/REDTEAM_REPORT.md}"
READINESS_FILE="${GENESIS_SELFHOST_READINESS_REPORT:-$ROOT_DIR/.genesis/perf/selfhost_readiness_report.json}"
REFRESH_READINESS="${GENESIS_PLANNING_REFRESH_READINESS:-1}"
DATE_OVERRIDE="${GENESIS_PLANNING_SYNC_DATE:-}"

usage() {
  cat <<'EOF'
Usage: scripts/sync_upgrade_plan_state.sh [options]

Options:
  --date <YYYY-MM-DD>       Override "Last updated" date (default: local current date)
  --refresh-readiness <0|1> Refresh readiness before redteam validation (default: 1)
  -h, --help                Show this help

This command atomically refreshes:
1. `upgrade_plan.md` metadata (`Last updated`, `Open checklist items`)
2. `docs/status/REDTEAM_REPORT.md` active P0/P1 risks from open plan items
3. `.genesis/perf/selfhost_readiness_report.json` (optional refresh)

It then runs integrity checks and fails if any artifact is inconsistent.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --date)
      DATE_OVERRIDE="${2:-}"
      shift 2
      ;;
    --refresh-readiness)
      REFRESH_READINESS="${2:-}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "sync-upgrade-plan-state: unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [[ "$REFRESH_READINESS" != "0" && "$REFRESH_READINESS" != "1" ]]; then
  echo "sync-upgrade-plan-state: --refresh-readiness must be 0 or 1" >&2
  exit 2
fi

[[ -f "$PLAN_FILE" ]] || {
  echo "sync-upgrade-plan-state: missing plan file: $PLAN_FILE" >&2
  exit 1
}

python3 - "$PLAN_FILE" "$REPORT_FILE" "$DATE_OVERRIDE" <<'PY'
import datetime as dt
import pathlib
import re
import sys

plan_path = pathlib.Path(sys.argv[1])
report_path = pathlib.Path(sys.argv[2])
date_override = sys.argv[3].strip()

if date_override:
    try:
        update_date = dt.date.fromisoformat(date_override)
    except ValueError as exc:
        raise SystemExit(
            f"sync-upgrade-plan-state: --date must be YYYY-MM-DD, got {date_override!r}"
        ) from exc
else:
    update_date = dt.date.today()

plan_text = plan_path.read_text(encoding="utf-8")
plan_lines = plan_text.splitlines()
open_items = []
open_count = 0
for line in plan_lines:
    if re.match(r"^- \[ \] ", line):
        open_count += 1
    m = re.match(r"^- \[ \] (P[01]\.\d+)\s+(.+)$", line)
    if m:
        open_items.append((m.group(1), m.group(2).strip()))

if re.search(r"^Last updated:\s+\d{4}-\d{2}-\d{2}$", plan_text, flags=re.MULTILINE) is None:
    raise SystemExit(
        "sync-upgrade-plan-state: upgrade_plan.md missing `Last updated: YYYY-MM-DD`"
    )
updated_plan = re.sub(
    r"^Last updated:\s+\d{4}-\d{2}-\d{2}$",
    f"Last updated: {update_date.isoformat()}",
    plan_text,
    count=1,
    flags=re.MULTILINE,
)

if re.search(r"^Open checklist items:\s+\d+$", updated_plan, flags=re.MULTILINE) is None:
    raise SystemExit(
        "sync-upgrade-plan-state: upgrade_plan.md missing `Open checklist items: <N>`"
    )
updated_plan_2 = re.sub(
    r"^Open checklist items:\s+\d+$",
    f"Open checklist items: {open_count}",
    updated_plan,
    count=1,
    flags=re.MULTILINE,
)

plan_path.write_text(updated_plan_2.rstrip() + "\n", encoding="utf-8")

header = [
    "# GenesisCode Red-Team Report (P0/P1 Active Risk Summary)",
    "",
    f"Last updated: {update_date.isoformat()}",
    "",
    "Scope:",
    f"- Track unresolved `P0`/`P1` risks from `{plan_path}`.",
    f"- Keep active IDs synchronized with `{plan_path.parent / '.genesis/perf/selfhost_readiness_report.json'}`.",
    "",
    "## Active Risks (P0/P1)",
    "",
]

if open_items:
    risks = [f"- {pid} - {summary}" for pid, summary in open_items]
else:
    risks = ["No active P0/P1 risks."]

report_text = "\n".join(header + risks).rstrip() + "\n"
report_path.parent.mkdir(parents=True, exist_ok=True)
report_path.write_text(report_text, encoding="utf-8")

print(
    "sync-upgrade-plan-state: refreshed "
    f"plan={plan_path} report={report_path} open_items={open_count} p0_p1={len(open_items)} date={update_date.isoformat()}"
)
PY

if [[ "$REFRESH_READINESS" == "1" ]]; then
  bash scripts/update_selfhost_readiness_scorecard_report.sh >/dev/null
elif [[ ! -f "$READINESS_FILE" ]]; then
  bash scripts/update_selfhost_readiness_scorecard_report.sh >/dev/null
fi

GENESIS_PLANNING_REFRESH_READINESS=0 bash scripts/check_redteam_report.sh
bash scripts/check_planning_docs_fresh.sh
bash scripts/check_doc_topology_drift.sh

echo "sync-upgrade-plan-state: ok"
