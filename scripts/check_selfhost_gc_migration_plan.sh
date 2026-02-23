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
module_paths = [str(x) for x in module_paths if isinstance(x, str) and x]

migration_rows = policy.get("migration_rows")
if not isinstance(migration_rows, list) or not migration_rows:
    raise SystemExit("selfhost-gc-migration-plan: policy migration_rows must be a non-empty table list")

required_min_phase_raw = str(policy.get("required_min_phase", "phase-1"))
phase_match = re.fullmatch(r"phase-(\d+)", required_min_phase_raw)
if phase_match is None:
    raise SystemExit("selfhost-gc-migration-plan: required_min_phase must match phase-<n>")
required_min_phase = int(phase_match.group(1))

disallowed_statuses_raw = policy.get("disallowed_statuses", [])
if not isinstance(disallowed_statuses_raw, list):
    raise SystemExit("selfhost-gc-migration-plan: disallowed_statuses must be a list")
disallowed_statuses = {str(x) for x in disallowed_statuses_raw if str(x)}
allowed_statuses = {"planned", "in-progress", "migrated", "blocked"}

policy_rows: dict[str, dict] = {}
for row in migration_rows:
    if not isinstance(row, dict):
        raise SystemExit("selfhost-gc-migration-plan: each migration_rows entry must be a table")
    module_path = row.get("module_path")
    targets = row.get("target_gc_modules")
    parity_gate = row.get("parity_gate")
    phase = row.get("phase")
    status = row.get("status")
    if not isinstance(module_path, str) or not module_path:
        raise SystemExit("selfhost-gc-migration-plan: migration_rows.module_path must be a non-empty string")
    if not isinstance(targets, list) or not targets or not all(isinstance(x, str) and x for x in targets):
        raise SystemExit(
            f"selfhost-gc-migration-plan: migration_rows[{module_path}].target_gc_modules must be a non-empty string list"
        )
    if not isinstance(parity_gate, str) or not parity_gate:
        raise SystemExit(f"selfhost-gc-migration-plan: migration_rows[{module_path}].parity_gate must be a non-empty string")
    if not isinstance(phase, str) or re.fullmatch(r"phase-(\d+)", phase) is None:
        raise SystemExit(f"selfhost-gc-migration-plan: migration_rows[{module_path}].phase must match phase-<n>")
    if not isinstance(status, str) or status not in allowed_statuses:
        raise SystemExit(
            f"selfhost-gc-migration-plan: migration_rows[{module_path}].status must be one of {sorted(allowed_statuses)}"
        )
    if module_path in policy_rows:
        raise SystemExit(f"selfhost-gc-migration-plan: duplicate migration_rows.module_path: {module_path}")
    policy_rows[module_path] = {
        "targets": sorted(set(targets)),
        "parity_gate": parity_gate.strip(),
        "phase": phase,
        "status": status,
    }

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
plan_rows: dict[str, dict] = {}
for m in row_pattern.finditer(plan_text):
    line = m.group(0)
    cells = [c.strip() for c in line.split("|")]
    if len(cells) < 6:
        continue
    first_cell = cells[1]
    path_match = re.search(r"`(crates/[^`]+\.rs)`", first_cell)
    if not path_match:
        continue
    module_path = path_match.group(1)

    target_cell = cells[2]
    parity_cell = cells[3]
    phase_cell = cells[4]
    status_cell = cells[5]

    phase_value_match = re.search(r"phase-(\d+)", phase_cell)
    status_value_match = re.search(r"(planned|in-progress|migrated|blocked)", status_cell)
    if phase_value_match is None or status_value_match is None:
        raise SystemExit(
            "selfhost-gc-migration-plan: invalid phase/status formatting for row: " + line
        )
    parity_gate_match = re.search(r"`([^`]+)`", parity_cell)
    if parity_gate_match is None:
        raise SystemExit(
            "selfhost-gc-migration-plan: invalid parity-gate formatting for row: " + line
        )
    targets = sorted(
        set(
            token
            for token in re.findall(r"`([^`]+)`", target_cell)
            if token.startswith("prelude/") or token.startswith("selfhost/")
        )
    )
    if not targets:
        raise SystemExit(
            "selfhost-gc-migration-plan: target module cell must include backticked GC module paths: " + line
        )

    row_paths.append(module_path)
    plan_rows[module_path] = {
        "targets": targets,
        "parity_gate": parity_gate_match.group(1).strip(),
        "phase": f"phase-{phase_value_match.group(1)}",
        "status": status_value_match.group(1),
    }

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

policy_missing_rows = sorted(set(policy_paths) - set(policy_rows.keys()))
policy_stale_rows = sorted(set(policy_rows.keys()) - set(policy_paths))
if policy_missing_rows:
    errors.append("policy-missing-migration-rows:" + ",".join(policy_missing_rows))
if policy_stale_rows:
    errors.append("policy-stale-migration-rows:" + ",".join(policy_stale_rows))

row_mismatches = []
regressions = []
for module_path in policy_paths:
    policy_row = policy_rows.get(module_path)
    plan_row = plan_rows.get(module_path)
    if policy_row is None or plan_row is None:
        continue

    if policy_row["targets"] != plan_row["targets"]:
        row_mismatches.append(
            f"{module_path}:targets policy={policy_row['targets']} plan={plan_row['targets']}"
        )
    if policy_row["parity_gate"] != plan_row["parity_gate"]:
        row_mismatches.append(
            f"{module_path}:parity policy={policy_row['parity_gate']} plan={plan_row['parity_gate']}"
        )
    if policy_row["phase"] != plan_row["phase"]:
        row_mismatches.append(
            f"{module_path}:phase policy={policy_row['phase']} plan={plan_row['phase']}"
        )
    if policy_row["status"] != plan_row["status"]:
        row_mismatches.append(
            f"{module_path}:status policy={policy_row['status']} plan={plan_row['status']}"
        )

    row_phase_match = re.fullmatch(r"phase-(\d+)", plan_row["phase"])
    if row_phase_match is None:
        regressions.append(f"{module_path}:invalid-phase:{plan_row['phase']}")
    else:
        row_phase = int(row_phase_match.group(1))
        if row_phase < required_min_phase:
            regressions.append(
                f"{module_path}:phase-regression:{plan_row['phase']}<phase-{required_min_phase}"
            )
    if plan_row["status"] in disallowed_statuses:
        regressions.append(f"{module_path}:disallowed-status:{plan_row['status']}")

if row_mismatches:
    errors.append("row-mismatch:" + " | ".join(row_mismatches))
if regressions:
    errors.append("migration-regression:" + " | ".join(regressions))

report = {
    "kind": "genesis/selfhost-gc-migration-plan-v0.1",
    "policy_path": policy_path.relative_to(root).as_posix(),
    "plan_path": plan_path.relative_to(root).as_posix(),
    "module_count": len(policy_paths),
    "plan_row_count": len(planned_paths),
    "policy_row_count": len(policy_rows),
    "required_min_phase": f"phase-{required_min_phase}",
    "disallowed_statuses": sorted(disallowed_statuses),
    "missing_from_plan": missing_from_plan,
    "stale_in_plan": stale_in_plan,
    "policy_missing_rows": policy_missing_rows,
    "policy_stale_rows": policy_stale_rows,
    "row_mismatches": row_mismatches,
    "regressions": regressions,
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
