#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

DOC_PATH="docs/spec/FULL_SELFHOST_CUTOVER_PROFILE_v0.1.md"
REPORT_PATH="${GENESIS_FULL_SELFHOST_CUTOVER_REPORT:-.genesis/perf/full_selfhost_cutover_profile_report.json}"
REFRESH="${GENESIS_FULL_SELFHOST_CUTOVER_REFRESH:-0}"
READINESS_REPORT="${GENESIS_SELFHOST_READINESS_REPORT:-.genesis/perf/selfhost_readiness_report.json}"
BOOTSTRAP_REPORT="${GENESIS_BOOTSTRAP_RETIREMENT_REPORT:-.genesis/perf/bootstrap_retirement_gate_report.json}"
DASHBOARD_FRESH_REPORT="${GENESIS_SELFHOST_DASHBOARD_FRESH_REPORT:-.genesis/perf/selfhost_dashboard_fresh_report.json}"

if [[ "$REFRESH" != "0" && "$REFRESH" != "1" ]]; then
  echo "full-selfhost-cutover-profile: GENESIS_FULL_SELFHOST_CUTOVER_REFRESH must be 0 or 1" >&2
  exit 2
fi

if [[ "$REFRESH" == "1" ]]; then
  bash scripts/check_selfhost_boundary.sh --strict
  bash scripts/check_bootstrap_retirement_gate.sh
  bash scripts/check_selfhost_dashboard_fresh.sh
  bash scripts/check_selfhost_readiness_scorecard.sh
fi

[[ -f "$DOC_PATH" ]] || {
  echo "full-selfhost-cutover-profile: missing doc: $DOC_PATH" >&2
  exit 1
}
[[ -f "$READINESS_REPORT" ]] || {
  echo "full-selfhost-cutover-profile: missing readiness report: $READINESS_REPORT" >&2
  exit 1
}
[[ -f "$BOOTSTRAP_REPORT" ]] || {
  echo "full-selfhost-cutover-profile: missing bootstrap retirement report: $BOOTSTRAP_REPORT" >&2
  exit 1
}
[[ -f "$DASHBOARD_FRESH_REPORT" ]] || {
  echo "full-selfhost-cutover-profile: missing selfhost dashboard freshness report: $DASHBOARD_FRESH_REPORT" >&2
  exit 1
}

python3 - "$ROOT_DIR" "$DOC_PATH" "$READINESS_REPORT" "$BOOTSTRAP_REPORT" "$DASHBOARD_FRESH_REPORT" "$REPORT_PATH" <<'PY'
import json
import pathlib
import re
import sys

root = pathlib.Path(sys.argv[1]).resolve()
doc_path = root / sys.argv[2]
readiness_path = root / sys.argv[3]
bootstrap_path = root / sys.argv[4]
dashboard_path = root / sys.argv[5]
report_path = root / sys.argv[6]

doc = doc_path.read_text(encoding="utf-8")

required_headings = [
    "# Full-Selfhost Cutover Profile v0.1",
    "## Remaining Exceptions (Explicit)",
    "## Closure Path",
    "## Gate Contract",
]
missing_headings = [h for h in required_headings if h not in doc]
if missing_headings:
    raise SystemExit(
        "full-selfhost-cutover-profile: missing required heading(s): "
        + ", ".join(missing_headings)
    )

required_exceptions = {
    "gc_coreform",
    "gc_kernel",
    "gc_prelude",
    "gc_effects",
    "gc_cli_driver",
}
found_exceptions = set(re.findall(r"- `([^`]+)`", doc))
missing_exceptions = sorted(required_exceptions - found_exceptions)
if missing_exceptions:
    raise SystemExit(
        "full-selfhost-cutover-profile: missing required explicit exceptions: "
        + ", ".join(missing_exceptions)
    )

if "scripts/check_full_selfhost_cutover_profile.sh" not in doc:
    raise SystemExit(
        "full-selfhost-cutover-profile: gate contract must reference scripts/check_full_selfhost_cutover_profile.sh"
    )
if "scripts/check_selfhost_boundary.sh --strict" not in doc:
    raise SystemExit(
        "full-selfhost-cutover-profile: gate contract must reference strict selfhost boundary guard"
    )

readiness = json.loads(readiness_path.read_text(encoding="utf-8"))
if readiness.get("kind") != "genesis/selfhost-readiness-v0.1":
    raise SystemExit(
        "full-selfhost-cutover-profile: readiness report kind mismatch"
    )

dimensions = readiness.get("dimensions")
if not isinstance(dimensions, dict) or not dimensions:
    raise SystemExit(
        "full-selfhost-cutover-profile: readiness report dimensions missing/invalid"
    )
dimension_failures = sorted(
    name for name, spec in dimensions.items() if not bool(spec.get("ok", False))
)
if dimension_failures:
    raise SystemExit(
        "full-selfhost-cutover-profile: readiness dimension(s) failing: "
        + ", ".join(dimension_failures)
    )

fail_reasons = readiness.get("fail_reasons", [])
if not isinstance(fail_reasons, list):
    raise SystemExit(
        "full-selfhost-cutover-profile: readiness fail_reasons must be a list"
    )
invalid_fail_reasons = sorted(
    {str(x) for x in fail_reasons if str(x) != "open-upgrade-plan-ids"}
)
if invalid_fail_reasons:
    raise SystemExit(
        "full-selfhost-cutover-profile: unsupported readiness fail reasons: "
        + ", ".join(invalid_fail_reasons)
    )

bootstrap = json.loads(bootstrap_path.read_text(encoding="utf-8"))
if bootstrap.get("kind") != "genesis/bootstrap-retirement-gate-report-v0.1":
    raise SystemExit(
        "full-selfhost-cutover-profile: bootstrap retirement report kind mismatch"
    )
bootstrap_status = str(bootstrap.get("status", ""))
if bootstrap_status not in {"ok", "degraded"}:
    raise SystemExit(
        "full-selfhost-cutover-profile: bootstrap retirement status must be ok|degraded"
    )

dashboard = json.loads(dashboard_path.read_text(encoding="utf-8"))
if dashboard.get("kind") != "genesis/selfhost-dashboard-fresh-v0.1":
    raise SystemExit(
        "full-selfhost-cutover-profile: dashboard freshness report kind mismatch"
    )
if not bool(dashboard.get("ok", False)):
    raise SystemExit(
        "full-selfhost-cutover-profile: dashboard freshness report is not ok"
    )

report_doc = {
    "kind": "genesis/full-selfhost-cutover-profile-v0.1",
    "doc": doc_path.relative_to(root).as_posix(),
    "readiness_report": readiness_path.relative_to(root).as_posix(),
    "bootstrap_report": bootstrap_path.relative_to(root).as_posix(),
    "dashboard_fresh_report": dashboard_path.relative_to(root).as_posix(),
    "explicit_exceptions": sorted(required_exceptions),
    "readiness_dimension_count": len(dimensions),
    "readiness_fail_reasons": [str(x) for x in fail_reasons],
    "bootstrap_status": bootstrap_status,
    "ok": True,
}
report_path.parent.mkdir(parents=True, exist_ok=True)
report_path.write_text(json.dumps(report_doc, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(
    "full-selfhost-cutover-profile: ok "
    f"(dimensions={len(dimensions)} bootstrap_status={bootstrap_status})"
)
PY
