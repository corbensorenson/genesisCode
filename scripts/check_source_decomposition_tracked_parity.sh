#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

POLICY_FILE="${GENESIS_SOURCE_DECOMPOSITION_POLICY:-policies/source_decomposition_progress.toml}"
REPORT_FILE="${GENESIS_SOURCE_DECOMPOSITION_TRACKED_PARITY_REPORT:-.genesis/perf/source_decomposition_tracked_parity_report.json}"
PARITY_RETRIES="${GENESIS_SOURCE_DECOMPOSITION_PARITY_RETRIES:-1}"

[[ -f "$POLICY_FILE" ]] || {
  echo "source-decomposition-tracked-parity: missing policy file: $POLICY_FILE" >&2
  exit 1
}
if [[ ! "$PARITY_RETRIES" =~ ^[0-9]+$ ]]; then
  echo "source-decomposition-tracked-parity: GENESIS_SOURCE_DECOMPOSITION_PARITY_RETRIES must be a non-negative integer" >&2
  exit 2
fi

python3 - "$ROOT_DIR" "$POLICY_FILE" "$REPORT_FILE" "$PARITY_RETRIES" <<'PY'
import json
import pathlib
import re
import subprocess
import sys

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover
    import tomli as tomllib

root = pathlib.Path(sys.argv[1]).resolve()
policy_path = root / sys.argv[2]
report_path = root / sys.argv[3]
max_retries = int(sys.argv[4])
policy = tomllib.loads(policy_path.read_text(encoding="utf-8"))

if policy.get("version") != 1:
    raise SystemExit("source-decomposition-tracked-parity: policy version must be 1")

target_max_lines = policy.get("target_max_lines")
if not isinstance(target_max_lines, int) or target_max_lines <= 0:
    raise SystemExit(
        "source-decomposition-tracked-parity: target_max_lines must be a positive integer"
    )

required_min_phase_raw = str(policy.get("required_min_phase", "phase-1"))
required_min_phase_match = re.fullmatch(r"phase-(\d+)", required_min_phase_raw)
if required_min_phase_match is None:
    raise SystemExit(
        "source-decomposition-tracked-parity: required_min_phase must match phase-<n>"
    )
required_min_phase = int(required_min_phase_match.group(1))

disallowed_statuses_raw = policy.get("disallowed_statuses", [])
if not isinstance(disallowed_statuses_raw, list):
    raise SystemExit(
        "source-decomposition-tracked-parity: disallowed_statuses must be a list"
    )
disallowed_statuses = {str(x) for x in disallowed_statuses_raw if str(x)}

tracked_rows_raw = policy.get("tracked_over_budget_rows", [])
if not isinstance(tracked_rows_raw, list):
    raise SystemExit(
        "source-decomposition-tracked-parity: tracked_over_budget_rows must be a list"
    )
if not tracked_rows_raw:
    raise SystemExit(
        "source-decomposition-tracked-parity: tracked_over_budget_rows must not be empty"
    )

allowed_statuses = {"planned", "in-progress", "migrated", "blocked", "waived"}
rows = []
regressions: list[str] = []
command_order: list[str] = []
seen_commands: set[str] = set()

for row in tracked_rows_raw:
    if not isinstance(row, dict):
        raise SystemExit(
            "source-decomposition-tracked-parity: tracked_over_budget_rows entries must be tables"
        )
    module_path = row.get("module_path")
    parity_gate = row.get("parity_gate")
    phase = row.get("phase")
    status = row.get("status")
    waiver_owner = row.get("waiver_owner")
    waiver_scope = row.get("waiver_scope")
    waiver_rationale = row.get("waiver_rationale")
    waiver_review_by = row.get("waiver_review_by")

    if not isinstance(module_path, str) or not module_path:
        raise SystemExit(
            "source-decomposition-tracked-parity: tracked_over_budget_rows.module_path must be a non-empty string"
        )
    if not isinstance(parity_gate, str) or not parity_gate.strip():
        raise SystemExit(
            f"source-decomposition-tracked-parity: tracked_over_budget_rows[{module_path}].parity_gate must be a non-empty string"
        )
    if not isinstance(phase, str):
        raise SystemExit(
            f"source-decomposition-tracked-parity: tracked_over_budget_rows[{module_path}].phase must be a string"
        )
    phase_match = re.fullmatch(r"phase-(\d+)", phase)
    if phase_match is None:
        raise SystemExit(
            f"source-decomposition-tracked-parity: tracked_over_budget_rows[{module_path}].phase must match phase-<n>"
        )
    phase_num = int(phase_match.group(1))
    if phase_num < required_min_phase:
        regressions.append(
            f"{module_path}:phase-regression:{phase}<phase-{required_min_phase}"
        )

    if not isinstance(status, str) or status not in allowed_statuses:
        raise SystemExit(
            f"source-decomposition-tracked-parity: tracked_over_budget_rows[{module_path}].status must be one of {sorted(allowed_statuses)}"
        )
    if status in disallowed_statuses:
        regressions.append(f"{module_path}:disallowed-status:{status}")

    if status == "waived":
        if not isinstance(waiver_owner, str) or not waiver_owner.strip():
            regressions.append(f"{module_path}:missing-waiver-owner")
        if not isinstance(waiver_scope, str) or not waiver_scope.strip():
            regressions.append(f"{module_path}:missing-waiver-scope")
        if not isinstance(waiver_rationale, str) or not waiver_rationale.strip():
            regressions.append(f"{module_path}:missing-waiver-rationale")
        if not isinstance(waiver_review_by, str) or re.fullmatch(
            r"\d{4}-\d{2}-\d{2}", waiver_review_by
        ) is None:
            regressions.append(f"{module_path}:invalid-waiver-review-by")
    elif any(x is not None for x in (waiver_owner, waiver_scope, waiver_rationale, waiver_review_by)):
        regressions.append(f"{module_path}:waiver-fields-present-without-waived-status")

    abs_path = root / module_path
    path_exists = abs_path.is_file()
    line_count = 0
    if path_exists:
        line_count = sum(1 for _ in abs_path.open("r", encoding="utf-8"))
        if line_count <= target_max_lines:
            regressions.append(
                f"{module_path}:stale-tracked-row:{line_count}<={target_max_lines}"
            )
    else:
        regressions.append(f"{module_path}:missing-path")

    gate = parity_gate.strip()
    if gate not in seen_commands:
        command_order.append(gate)
        seen_commands.add(gate)

    rows.append(
        {
            "path": module_path,
            "exists": path_exists,
            "lines": line_count if path_exists else None,
            "phase": phase,
            "status": status,
            "parity_gate": gate,
            "waiver_owner": waiver_owner.strip() if isinstance(waiver_owner, str) else "",
            "waiver_scope": waiver_scope.strip() if isinstance(waiver_scope, str) else "",
            "waiver_rationale": waiver_rationale.strip() if isinstance(waiver_rationale, str) else "",
            "waiver_review_by": waiver_review_by.strip() if isinstance(waiver_review_by, str) else "",
        }
    )

command_results = []
command_failures = []
for command in command_order:
    attempts = []
    ok = False
    for attempt in range(1, max_retries + 2):
        proc = subprocess.run(
            ["bash", "-lc", command],
            cwd=root,
            capture_output=True,
            text=True,
        )
        attempt_doc = {
            "attempt": attempt,
            "exit_code": proc.returncode,
            "stdout_tail": (proc.stdout or "").strip()[-800:],
            "stderr_tail": (proc.stderr or "").strip()[-800:],
        }
        attempts.append(attempt_doc)
        if proc.returncode == 0:
            ok = True
            break

    final_attempt = attempts[-1] if attempts else {"exit_code": 1, "stdout_tail": "", "stderr_tail": ""}
    command_results.append(
        {
            "command": command,
            "ok": ok,
            "attempts": attempts,
            "exit_code": final_attempt["exit_code"],
            "stdout_tail": final_attempt["stdout_tail"],
            "stderr_tail": final_attempt["stderr_tail"],
        }
    )
    if not ok:
        command_failures.append(f"{command}:exit={final_attempt['exit_code']}")

errors: list[str] = []
if regressions:
    errors.append("policy-regression:" + " | ".join(regressions))
if command_failures:
    errors.append("parity-gate-failures:" + " | ".join(command_failures))

report = {
    "kind": "genesis/source-decomposition-tracked-parity-v0.1",
    "policy_path": policy_path.relative_to(root).as_posix(),
    "target_max_lines": target_max_lines,
    "required_min_phase": f"phase-{required_min_phase}",
    "disallowed_statuses": sorted(disallowed_statuses),
    "tracked_row_count": len(rows),
    "parity_retries": max_retries,
    "rows": rows,
    "commands": command_results,
    "regressions": regressions,
    "errors": errors,
    "ok": not errors,
}
report_path.parent.mkdir(parents=True, exist_ok=True)
report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")

if errors:
    raise SystemExit("source-decomposition-tracked-parity: " + " | ".join(errors))

print(
    "source-decomposition-tracked-parity: ok "
    f"(rows={len(rows)} commands={len(command_order)})"
)
PY
