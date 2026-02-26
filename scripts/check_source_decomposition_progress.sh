#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

POLICY_FILE="${GENESIS_SOURCE_DECOMPOSITION_POLICY:-policies/source_decomposition_progress.toml}"
REPORT_FILE="${GENESIS_SOURCE_DECOMPOSITION_REPORT:-.genesis/perf/source_decomposition_progress_report.json}"

[[ -f "$POLICY_FILE" ]] || {
  echo "source-decomposition-progress: missing policy file: $POLICY_FILE" >&2
  exit 1
}

python3 - "$ROOT_DIR" "$POLICY_FILE" "$REPORT_FILE" <<'PY'
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
report_path = root / sys.argv[3]
policy = tomllib.loads(policy_path.read_text(encoding="utf-8"))

if policy.get("version") != 1:
    raise SystemExit("source-decomposition-progress: policy version must be 1")

required_min_phase_raw = str(policy.get("required_min_phase", "phase-1"))
required_min_phase_match = re.fullmatch(r"phase-(\d+)", required_min_phase_raw)
if required_min_phase_match is None:
    raise SystemExit("source-decomposition-progress: required_min_phase must match phase-<n>")
required_min_phase = int(required_min_phase_match.group(1))

disallowed_statuses_raw = policy.get("disallowed_statuses", [])
if not isinstance(disallowed_statuses_raw, list):
    raise SystemExit("source-decomposition-progress: disallowed_statuses must be a list")
disallowed_statuses = {str(x) for x in disallowed_statuses_raw if str(x)}

target = policy.get("target_max_lines")
if not isinstance(target, int) or target <= 0:
    raise SystemExit("source-decomposition-progress: target_max_lines must be a positive integer")

paths = policy.get("module_paths")
if not isinstance(paths, list) or not paths:
    raise SystemExit("source-decomposition-progress: module_paths must be a non-empty list")

coverage_paths = policy.get("coverage_module_paths")
if coverage_paths is None:
    coverage_paths = paths
if not isinstance(coverage_paths, list) or not coverage_paths:
    raise SystemExit(
        "source-decomposition-progress: coverage_module_paths must be a non-empty list"
    )

tracked_rows_raw = policy.get("tracked_over_budget_rows", [])
if not isinstance(tracked_rows_raw, list):
    raise SystemExit(
        "source-decomposition-progress: tracked_over_budget_rows must be a list"
    )

tracked_rows: dict[str, dict] = {}
regressions: list[str] = []
for row in tracked_rows_raw:
    if not isinstance(row, dict):
        raise SystemExit(
            "source-decomposition-progress: tracked_over_budget_rows entries must be tables"
        )
    module_path = row.get("module_path")
    target_gc_modules = row.get("target_gc_modules")
    parity_gate = row.get("parity_gate")
    phase = row.get("phase")
    status = row.get("status")
    notes = row.get("notes")

    if not isinstance(module_path, str) or not module_path:
        raise SystemExit(
            "source-decomposition-progress: tracked_over_budget_rows.module_path must be a non-empty string"
        )
    if not isinstance(target_gc_modules, list) or not target_gc_modules or not all(
        isinstance(x, str) and x for x in target_gc_modules
    ):
        raise SystemExit(
            "source-decomposition-progress: tracked_over_budget_rows.target_gc_modules must be a non-empty string list"
        )
    if not isinstance(parity_gate, str) or not parity_gate:
        raise SystemExit(
            "source-decomposition-progress: tracked_over_budget_rows.parity_gate must be a non-empty string"
        )
    if not isinstance(phase, str) or not phase.startswith("phase-"):
        raise SystemExit(
            "source-decomposition-progress: tracked_over_budget_rows.phase must use phase-<n> format"
        )
    if not isinstance(status, str) or status not in {"planned", "in-progress", "migrated", "blocked", "waived"}:
        raise SystemExit(
            "source-decomposition-progress: tracked_over_budget_rows.status must be one of planned|in-progress|migrated|blocked|waived"
        )
    if notes is not None and not isinstance(notes, str):
        raise SystemExit(
            "source-decomposition-progress: tracked_over_budget_rows.notes must be a string when provided"
        )
    phase_match = re.fullmatch(r"phase-(\d+)", phase)
    if phase_match is None:
        raise SystemExit(
            "source-decomposition-progress: tracked_over_budget_rows.phase must use phase-<n> format"
        )
    phase_num = int(phase_match.group(1))
    if phase_num < required_min_phase:
        regressions.append(
            f"{module_path}:phase-regression:{phase}<phase-{required_min_phase}"
        )
    if status in disallowed_statuses:
        regressions.append(f"{module_path}:disallowed-status:{status}")

    waiver_owner = row.get("waiver_owner")
    waiver_scope = row.get("waiver_scope")
    waiver_rationale = row.get("waiver_rationale")
    waiver_review_by = row.get("waiver_review_by")
    if status == "waived":
        if not isinstance(waiver_owner, str) or not waiver_owner.strip():
            raise SystemExit(
                "source-decomposition-progress: tracked_over_budget_rows.waiver_owner must be a non-empty string when status=waived"
            )
        if not isinstance(waiver_scope, str) or not waiver_scope.strip():
            raise SystemExit(
                "source-decomposition-progress: tracked_over_budget_rows.waiver_scope must be a non-empty string when status=waived"
            )
        if not isinstance(waiver_rationale, str) or not waiver_rationale.strip():
            raise SystemExit(
                "source-decomposition-progress: tracked_over_budget_rows.waiver_rationale must be a non-empty string when status=waived"
            )
        if not isinstance(waiver_review_by, str) or re.fullmatch(r"\d{4}-\d{2}-\d{2}", waiver_review_by) is None:
            raise SystemExit(
                "source-decomposition-progress: tracked_over_budget_rows.waiver_review_by must use YYYY-MM-DD when status=waived"
            )
    elif any(x is not None for x in (waiver_owner, waiver_scope, waiver_rationale, waiver_review_by)):
        regressions.append(f"{module_path}:waiver-fields-present-without-waived-status")

    if module_path in tracked_rows:
        raise SystemExit(
            f"source-decomposition-progress: duplicate tracked_over_budget_rows.module_path: {module_path}"
        )
    tracked_rows[module_path] = {
        "target_gc_modules": sorted(set(target_gc_modules)),
        "parity_gate": parity_gate,
        "phase": phase,
        "status": status,
        "notes": notes or "",
        "waiver_owner": waiver_owner.strip() if isinstance(waiver_owner, str) else "",
        "waiver_scope": waiver_scope.strip() if isinstance(waiver_scope, str) else "",
        "waiver_rationale": waiver_rationale.strip() if isinstance(waiver_rationale, str) else "",
        "waiver_review_by": waiver_review_by.strip() if isinstance(waiver_review_by, str) else "",
    }

coverage_rows = []
coverage_errors = []
coverage_set = set()
for rel in coverage_paths:
    if not isinstance(rel, str) or not rel:
        raise SystemExit(
            "source-decomposition-progress: coverage_module_paths entries must be non-empty strings"
        )
    if rel in coverage_set:
        coverage_errors.append(f"coverage-duplicate:{rel}")
        continue
    coverage_set.add(rel)
    abs_path = root / rel
    if not abs_path.is_file():
        coverage_errors.append(f"coverage-missing:{rel}")
        coverage_rows.append({"path": rel, "exists": False})
        continue
    coverage_rows.append({"path": rel, "exists": True})

rows = []
errors = []
for rel in paths:
    if not isinstance(rel, str) or not rel:
        raise SystemExit("source-decomposition-progress: module_paths entries must be non-empty strings")
    abs_path = root / rel
    if not abs_path.is_file():
        errors.append(f"missing:{rel}")
        rows.append({"path": rel, "exists": False, "lines": None, "ok": False})
        continue
    lines = sum(1 for _ in abs_path.open("r", encoding="utf-8"))
    ok = lines <= target
    if not ok:
        errors.append(f"over-budget:{rel}:{lines}>{target}")
    rows.append({"path": rel, "exists": True, "lines": lines, "ok": ok})

untracked_over_budget = []
tracked_over_budget = []
for abs_path in sorted((root / "crates").rglob("src/**/*.rs")):
    rel = abs_path.relative_to(root).as_posix()
    name = abs_path.name
    parent_parts = abs_path.relative_to(root).parts[:-1]
    if "/tests/" in rel or "/examples/" in rel:
        continue
    if any("test" in part for part in parent_parts):
        continue
    if "test" in name:
        continue
    lines = sum(1 for _ in abs_path.open("r", encoding="utf-8"))
    if lines <= target:
        continue
    if rel not in coverage_set:
        tracked = tracked_rows.get(rel)
        if tracked is None:
            untracked_over_budget.append({"path": rel, "lines": lines})
            errors.append(f"untracked-over-budget:{rel}:{lines}>{target}")
        else:
            tracked_over_budget.append(
                {
                    "path": rel,
                    "lines": lines,
                    "target_gc_modules": tracked["target_gc_modules"],
                    "parity_gate": tracked["parity_gate"],
                    "phase": tracked["phase"],
                    "status": tracked["status"],
                    "notes": tracked["notes"],
                    "waiver_owner": tracked["waiver_owner"],
                    "waiver_scope": tracked["waiver_scope"],
                    "waiver_rationale": tracked["waiver_rationale"],
                    "waiver_review_by": tracked["waiver_review_by"],
                }
            )

for rel, tracked in sorted(tracked_rows.items()):
    abs_path = root / rel
    if not abs_path.is_file():
        errors.append(f"tracked-over-budget-missing:{rel}")
        continue
    lines = sum(1 for _ in abs_path.open("r", encoding="utf-8"))
    if lines <= target:
        errors.append(f"tracked-over-budget-stale:{rel}:{lines}<={target}")

errors.extend(coverage_errors)
if regressions:
    errors.append("policy-regression:" + " | ".join(regressions))

report = {
    "kind": "genesis/source-decomposition-progress-v0.1",
    "policy_path": policy_path.relative_to(root).as_posix(),
    "target_max_lines": target,
    "required_min_phase": f"phase-{required_min_phase}",
    "disallowed_statuses": sorted(disallowed_statuses),
    "module_count": len(rows),
    "coverage_module_count": len(coverage_rows),
    "ok": not errors,
    "errors": errors,
    "modules": rows,
    "coverage_modules": coverage_rows,
    "tracked_over_budget_modules": tracked_over_budget,
    "untracked_over_budget_modules": untracked_over_budget,
    "regressions": regressions,
}
report_path.parent.mkdir(parents=True, exist_ok=True)
report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")

if errors:
    raise SystemExit("source-decomposition-progress: " + " | ".join(errors))

max_lines = max((row["lines"] or 0 for row in rows), default=0)
print(
    "source-decomposition-progress: ok "
    f"(modules={len(rows)} target_max_lines={target} observed_max_lines={max_lines})"
)
PY
