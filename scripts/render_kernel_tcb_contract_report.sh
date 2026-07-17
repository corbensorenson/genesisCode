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
import re
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

file_roles = policy.get("file_roles")
allowed_roles = {
    "reference-semantics",
    "shared-semantics",
    "tier-bridge",
    "optimized-tier",
}
if not isinstance(file_roles, dict) or not file_roles:
    raise SystemExit("kernel-tcb-contract: file_roles must be a non-empty table")
for rel, role in file_roles.items():
    if not isinstance(rel, str) or not rel or role not in allowed_roles:
        raise SystemExit(f"kernel-tcb-contract: invalid file role: {rel}={role}")

forbidden_role_markers = policy.get("forbidden_role_markers")
if (
    not isinstance(forbidden_role_markers, dict)
    or set(forbidden_role_markers) != {"reference-semantics", "optimized-tier"}
):
    raise SystemExit("kernel-tcb-contract: forbidden_role_markers role set mismatch")
for role, markers in forbidden_role_markers.items():
    if (
        not isinstance(markers, list)
        or not markers
        or not all(isinstance(x, str) and x for x in markers)
    ):
        raise SystemExit(
            f"kernel-tcb-contract: forbidden_role_markers[{role}] must be a non-empty string list"
        )

differential_test_file = policy.get("differential_test_file")
required_differential_test_markers = policy.get("required_differential_test_markers")
if not isinstance(differential_test_file, str) or not differential_test_file:
    raise SystemExit("kernel-tcb-contract: differential_test_file must be a non-empty path")
if (
    not isinstance(required_differential_test_markers, list)
    or not required_differential_test_markers
    or not all(isinstance(x, str) and x for x in required_differential_test_markers)
):
    raise SystemExit(
        "kernel-tcb-contract: required_differential_test_markers must be a non-empty string list"
    )

retired_kernel_paths = policy.get("retired_kernel_paths")
if (
    not isinstance(retired_kernel_paths, list)
    or not retired_kernel_paths
    or not all(isinstance(x, str) and x for x in retired_kernel_paths)
):
    raise SystemExit("kernel-tcb-contract: retired_kernel_paths must be a non-empty string list")
optimized_runtime_dir = policy.get("optimized_runtime_dir")
if not isinstance(optimized_runtime_dir, str) or not optimized_runtime_dir:
    raise SystemExit("kernel-tcb-contract: optimized_runtime_dir must be a non-empty path")
forbidden_optimized_runtime_patterns = policy.get("forbidden_optimized_runtime_patterns")
if (
    not isinstance(forbidden_optimized_runtime_patterns, list)
    or not forbidden_optimized_runtime_patterns
    or not all(isinstance(x, str) and x for x in forbidden_optimized_runtime_patterns)
):
    raise SystemExit(
        "kernel-tcb-contract: forbidden_optimized_runtime_patterns must be a non-empty string list"
    )
try:
    compiled_runtime_patterns = [
        re.compile(pattern) for pattern in forbidden_optimized_runtime_patterns
    ]
except re.error as error:
    raise SystemExit(f"kernel-tcb-contract: invalid optimized runtime pattern: {error}")

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

present_retired_paths = sorted(
    rel for rel in retired_kernel_paths if (kernel_src / rel).exists() or rel in expected_set
)
if present_retired_paths:
    errors.append("retired-kernel-paths-present:" + ",".join(present_retired_paths))

optimized_runtime_root = kernel_src / optimized_runtime_dir
if not optimized_runtime_root.is_dir():
    errors.append(f"missing-optimized-runtime-dir:{optimized_runtime_dir}")
optimized_runtime_pattern_rows = []
for path in sorted(optimized_runtime_root.rglob("*.rs")) if optimized_runtime_root.is_dir() else []:
    rel = path.relative_to(kernel_src).as_posix()
    source = path.read_text(encoding="utf-8")
    matches = [
        pattern.pattern for pattern in compiled_runtime_patterns if pattern.search(source)
    ]
    if matches:
        errors.append(f"forbidden-optimized-runtime-patterns:{rel}:" + "|".join(matches))
    optimized_runtime_pattern_rows.append({"path": rel, "matched_patterns": matches})

declared_role_paths = set(file_roles)
missing_role_paths = sorted(expected_set - declared_role_paths)
stale_role_paths = sorted(declared_role_paths - expected_set)
if missing_role_paths:
    errors.append("missing-file-roles:" + ",".join(missing_role_paths))
if stale_role_paths:
    errors.append("stale-file-roles:" + ",".join(stale_role_paths))

role_counts = {role: 0 for role in sorted(allowed_roles)}
role_marker_rows = []
for rel, role in sorted(file_roles.items()):
    role_counts[role] += 1
    if role == "reference-semantics" and rel.startswith("compiled"):
        errors.append(f"reference-role-uses-compiled-path:{rel}")
    if role == "optimized-tier" and not rel.startswith("compiled"):
        errors.append(f"optimized-role-escaped-compiled-path:{rel}")
    path = kernel_src / rel
    text = path.read_text(encoding="utf-8") if path.is_file() else ""
    present = [
        marker
        for marker in forbidden_role_markers.get(role, [])
        if marker in text
    ]
    if present:
        errors.append(f"forbidden-role-markers:{rel}:" + "|".join(present))
    role_marker_rows.append(
        {"path": rel, "role": role, "present_forbidden_markers": present}
    )

differential_path = root / differential_test_file
if not differential_path.is_file():
    errors.append(f"missing-differential-test-file:{differential_test_file}")
    differential_text = ""
else:
    differential_text = differential_path.read_text(encoding="utf-8")
missing_differential_markers = [
    marker
    for marker in required_differential_test_markers
    if marker not in differential_text
]
if missing_differential_markers:
    errors.append("missing-differential-test-markers:" + "|".join(missing_differential_markers))

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
    "retired_kernel_paths": retired_kernel_paths,
    "present_retired_paths": present_retired_paths,
    "optimized_runtime_dir": optimized_runtime_dir,
    "forbidden_optimized_runtime_patterns": forbidden_optimized_runtime_patterns,
    "optimized_runtime_pattern_rows": optimized_runtime_pattern_rows,
    "file_roles": role_marker_rows,
    "role_counts": role_counts,
    "missing_role_paths": missing_role_paths,
    "stale_role_paths": stale_role_paths,
    "forbidden_role_markers": forbidden_role_markers,
    "differential_test_file": differential_test_file,
    "required_differential_test_markers": required_differential_test_markers,
    "missing_differential_test_markers": missing_differential_markers,
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
    f"(surface_files={len(observed_files)} reference={role_counts['reference-semantics']} "
    f"shared={role_counts['shared-semantics']} bridge={role_counts['tier-bridge']} "
    f"optimized={role_counts['optimized-tier']} max_lines={max_lines} report={report_display})"
)
PY
