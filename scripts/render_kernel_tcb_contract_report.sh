#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

POLICY_FILE="${GENESIS_KERNEL_TCB_POLICY:-policies/kernel_tcb_contract.toml}"
REPORT_FILE="${1:?usage: scripts/render_kernel_tcb_contract_report.sh <report.json>}"

[[ -f "$POLICY_FILE" ]] || {
  echo "kernel-tcb-contract: missing policy file: $POLICY_FILE" >&2
  exit 1
}

python3 - "$ROOT_DIR" "$POLICY_FILE" "$REPORT_FILE" <<'PY'
import json
import pathlib
import sys
root = pathlib.Path(sys.argv[1]).resolve()
sys.path.insert(0, str(root / "scripts/lib"))
from toml_compat import tomllib
policy_path = root / sys.argv[2]
report_path = root / sys.argv[3]
policy = tomllib.loads(policy_path.read_text(encoding="utf-8"))

if policy.get("version") != 1:
    raise SystemExit("kernel-tcb-contract: policy version must be 1")

kernel_src_rel = policy.get("kernel_src_dir")
if not isinstance(kernel_src_rel, str) or not kernel_src_rel:
    raise SystemExit("kernel-tcb-contract: kernel_src_dir must be a non-empty string")
kernel_src = root / kernel_src_rel
if not kernel_src.is_dir():
    raise SystemExit(f"kernel-tcb-contract: kernel_src_dir not found: {kernel_src_rel}")

expected_files = policy.get("expected_kernel_surface_files")
if (
    not isinstance(expected_files, list)
    or not expected_files
    or not all(isinstance(x, str) and x for x in expected_files)
):
    raise SystemExit(
        "kernel-tcb-contract: expected_kernel_surface_files must be a non-empty string list"
    )
expected_set = set(expected_files)

required_eval_markers = policy.get("required_eval_markers")
if (
    not isinstance(required_eval_markers, list)
    or not required_eval_markers
    or not all(isinstance(x, str) and x for x in required_eval_markers)
):
    raise SystemExit("kernel-tcb-contract: required_eval_markers must be a non-empty string list")

forbidden_eval_markers = policy.get("forbidden_eval_markers")
if (
    not isinstance(forbidden_eval_markers, list)
    or not all(isinstance(x, str) and x for x in forbidden_eval_markers)
):
    raise SystemExit("kernel-tcb-contract: forbidden_eval_markers must be a string list")

line_budgets = policy.get("line_budgets")
if not isinstance(line_budgets, dict) or not line_budgets:
    raise SystemExit("kernel-tcb-contract: line_budgets must be a non-empty table")

errors: list[str] = []

# Surface file set enforcement.
observed_files = sorted(
    p.relative_to(kernel_src).as_posix()
    for p in kernel_src.rglob("*.rs")
    if p.name != "tests.rs" and "tests" not in p.relative_to(kernel_src).parts
)
observed_set = set(observed_files)
missing_surface = sorted(expected_set - observed_set)
extra_surface = sorted(observed_set - expected_set)
if missing_surface:
    errors.append("missing-surface-files:" + ",".join(missing_surface))
if extra_surface:
    errors.append("extra-surface-files:" + ",".join(extra_surface))

expected_budget_paths = {
    (pathlib.Path(kernel_src_rel) / rel).as_posix() for rel in expected_set
}
declared_budget_paths = set(line_budgets)
missing_budget_paths = sorted(expected_budget_paths - declared_budget_paths)
stale_budget_paths = sorted(declared_budget_paths - expected_budget_paths)
if missing_budget_paths:
    errors.append("missing-line-budgets:" + ",".join(missing_budget_paths))
if stale_budget_paths:
    errors.append("stale-line-budgets:" + ",".join(stale_budget_paths))

# Line-budget enforcement.
line_rows = []
for rel, budget in sorted(line_budgets.items()):
    if not isinstance(rel, str) or not rel:
        raise SystemExit("kernel-tcb-contract: line_budgets keys must be non-empty strings")
    if not isinstance(budget, int) or budget <= 0:
        raise SystemExit(
            f"kernel-tcb-contract: line_budgets[{rel}] must be a positive integer"
        )
    path = root / rel
    if not path.is_file():
        errors.append(f"missing-budget-path:{rel}")
        line_rows.append(
            {"path": rel, "exists": False, "budget": budget, "lines": None, "ok": False}
        )
        continue
    lines = sum(1 for _ in path.open("r", encoding="utf-8"))
    ok = lines <= budget
    if not ok:
        errors.append(f"line-budget-exceeded:{rel}:{lines}>{budget}")
    line_rows.append(
        {"path": rel, "exists": True, "budget": budget, "lines": lines, "ok": ok}
    )

eval_rs = root / "crates/gc_kernel/src/eval.rs"
if not eval_rs.is_file():
    errors.append("missing-eval-rs:crates/gc_kernel/src/eval.rs")
    eval_text = ""
else:
    eval_text = eval_rs.read_text(encoding="utf-8")

missing_required_markers = [m for m in required_eval_markers if m not in eval_text]
present_forbidden_markers = [m for m in forbidden_eval_markers if m in eval_text]
if missing_required_markers:
    errors.append("missing-required-markers:" + " | ".join(missing_required_markers))
if present_forbidden_markers:
    errors.append("forbidden-markers-present:" + " | ".join(present_forbidden_markers))

report = {
    "kind": "genesis/kernel-tcb-contract-v0.1",
    "policy_path": policy_path.relative_to(root).as_posix(),
    "kernel_src_dir": kernel_src_rel,
    "ok": not errors,
    "errors": errors,
    "expected_surface_files": sorted(expected_set),
    "observed_surface_files": observed_files,
    "missing_surface_files": missing_surface,
    "extra_surface_files": extra_surface,
    "missing_line_budget_paths": missing_budget_paths,
    "stale_line_budget_paths": stale_budget_paths,
    "line_budgets": line_rows,
    "required_eval_markers": required_eval_markers,
    "forbidden_eval_markers": forbidden_eval_markers,
    "missing_required_markers": missing_required_markers,
    "present_forbidden_markers": present_forbidden_markers,
}
report_path.parent.mkdir(parents=True, exist_ok=True)
report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")

if errors:
    raise SystemExit("kernel-tcb-contract: " + " | ".join(errors))

max_lines = max((row["lines"] or 0 for row in line_rows), default=0)
try:
    report_display = report_path.relative_to(root).as_posix()
except ValueError:
    report_display = str(report_path)
print(
    "kernel-tcb-contract: ok "
    f"(surface_files={len(observed_files)} max_lines={max_lines} report={report_display})"
)
PY
