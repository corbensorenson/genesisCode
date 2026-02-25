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
        untracked_over_budget.append({"path": rel, "lines": lines})
        errors.append(f"untracked-over-budget:{rel}:{lines}>{target}")

errors.extend(coverage_errors)

report = {
    "kind": "genesis/source-decomposition-progress-v0.1",
    "policy_path": policy_path.relative_to(root).as_posix(),
    "target_max_lines": target,
    "module_count": len(rows),
    "coverage_module_count": len(coverage_rows),
    "ok": not errors,
    "errors": errors,
    "modules": rows,
    "coverage_modules": coverage_rows,
    "untracked_over_budget_modules": untracked_over_budget,
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
