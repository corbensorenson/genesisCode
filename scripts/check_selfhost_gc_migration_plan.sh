#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

POLICY_FILE="policies/source_decomposition_progress.toml"
PLAN_FILE="docs/spec/GC_MODULE_BOUNDARIES_v0.1.md"
REPORT_FILE="${GENESIS_SELFHOST_GC_MIGRATION_PLAN_REPORT:-.genesis/perf/selfhost_gc_migration_plan_report.json}"

[[ -f "$POLICY_FILE" ]] || {
  echo "selfhost-gc-migration-plan: missing policy file: $POLICY_FILE" >&2
  exit 1
}
[[ -f "$PLAN_FILE" ]] || {
  echo "selfhost-gc-migration-plan: missing plan file: $PLAN_FILE" >&2
  exit 1
}

python3 - "$ROOT_DIR" "$POLICY_FILE" "$PLAN_FILE" "$REPORT_FILE" <<'PY'
import json
import pathlib
import re
import sys

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover
    import tomli as tomllib

root = pathlib.Path(sys.argv[1]).resolve()
policy_path = root / sys.argv[2]
plan_path = root / sys.argv[3]
report_path = root / sys.argv[4]

policy = tomllib.loads(policy_path.read_text(encoding="utf-8"))
if policy.get("version") != 1:
    raise SystemExit("selfhost-gc-migration-plan: policy version must be 1")

module_paths = policy.get("module_paths")
if not isinstance(module_paths, list) or not module_paths:
    raise SystemExit("selfhost-gc-migration-plan: policy module_paths must be a non-empty list")

plan_text = plan_path.read_text(encoding="utf-8")
required_sections = [
    "## Selfhost Migration Plan (High-Churn Rust -> GC)",
    "Phase model:",
    "Exit criteria:",
]
missing_sections = [s for s in required_sections if s not in plan_text]
if missing_sections:
    raise SystemExit(
        "selfhost-gc-migration-plan: missing required section(s): " + ", ".join(missing_sections)
    )

row_paths: list[str] = []
row_pattern = re.compile(r"^\|.*\|\s*$", re.MULTILINE)
for m in row_pattern.finditer(plan_text):
    line = m.group(0)
    cells = [c.strip() for c in line.split("|")]
    if len(cells) < 6:
        continue
    first_cell = cells[1]
    path_match = re.search(r"`(crates/[^`]+\.rs)`", first_cell)
    if not path_match:
        continue
    phase_ok = re.search(r"`phase-[0-9]+`", line) is not None
    status_ok = re.search(r"`(planned|in-progress|migrated|blocked)`", line) is not None
    if not phase_ok or not status_ok:
        raise SystemExit(
            "selfhost-gc-migration-plan: invalid phase/status formatting for row: " + line
        )
    row_paths.append(path_match.group(1))

if not row_paths:
    raise SystemExit("selfhost-gc-migration-plan: migration map has no valid rows")

planned_paths = sorted(set(row_paths))
policy_paths = sorted({str(x) for x in module_paths if isinstance(x, str) and x})

missing_from_plan = sorted(set(policy_paths) - set(planned_paths))
stale_in_plan = sorted(set(planned_paths) - set(policy_paths))

errors = []
if missing_from_plan:
    errors.append("missing-in-plan:" + ",".join(missing_from_plan))
if stale_in_plan:
    errors.append("stale-in-plan:" + ",".join(stale_in_plan))

report = {
    "kind": "genesis/selfhost-gc-migration-plan-v0.1",
    "policy_path": policy_path.relative_to(root).as_posix(),
    "plan_path": plan_path.relative_to(root).as_posix(),
    "module_count": len(policy_paths),
    "plan_row_count": len(planned_paths),
    "missing_from_plan": missing_from_plan,
    "stale_in_plan": stale_in_plan,
    "ok": not errors,
    "errors": errors,
}
report_path.parent.mkdir(parents=True, exist_ok=True)
report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")

if errors:
    raise SystemExit("selfhost-gc-migration-plan: " + " | ".join(errors))

print(
    "selfhost-gc-migration-plan: ok "
    f"(modules={len(policy_paths)} rows={len(planned_paths)})"
)
PY
