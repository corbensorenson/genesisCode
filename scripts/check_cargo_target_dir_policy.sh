#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

REPORT_OUT="${GENESIS_CARGO_TARGET_POLICY_REPORT_OUT:-.genesis/perf/cargo_target_dir_policy_report.json}"
HISTORY_OUT="${GENESIS_CARGO_TARGET_POLICY_HISTORY_OUT:-.genesis/perf/cargo_target_dir_policy_history.jsonl}"

python3 - "$ROOT_DIR" "$REPORT_OUT" "$HISTORY_OUT" <<'PY'
import json
import pathlib
import re
import sys
import time

root = pathlib.Path(sys.argv[1])
report_path = pathlib.Path(sys.argv[2])
history_path = pathlib.Path(sys.argv[3])

scripts_dir = root / "scripts"
if not scripts_dir.is_dir():
    raise SystemExit(f"cargo-target-dir-policy: missing scripts directory: {scripts_dir}")

cargo_re = re.compile(r"(^|[ \t])cargo[ \t]+", re.MULTILINE)

# check_upgrade_plan_health.sh intentionally sets/export CARGO_TARGET_DIR directly
# for profile-wide orchestration instead of using the helper.
allow_direct_export = {"check_upgrade_plan_health.sh"}
allow_string_only = {"check_test_execution_profile_matrix.sh"}

scanned = 0
cargo_scripts = 0
violations = []

for path in sorted(scripts_dir.glob("*.sh")):
    scanned += 1
    text = path.read_text(encoding="utf-8")
    if cargo_re.search(text) is None:
        continue
    if path.name in allow_string_only:
        continue
    cargo_scripts += 1
    has_helper = "genesis_configure_cargo_target_dir" in text
    has_direct_export = (
        path.name in allow_direct_export and 'export CARGO_TARGET_DIR="' in text
    )
    if not has_helper and not has_direct_export:
        violations.append(path.name)

doc = {
    "kind": "genesis/cargo-target-dir-policy-v0.1",
    "timestamp_unix_s": int(time.time()),
    "scanned_scripts": scanned,
    "cargo_scripts": cargo_scripts,
    "violations": violations,
    "violation_count": len(violations),
    "ok": len(violations) == 0,
}

if report_path.is_file():
    try:
        prev = json.loads(report_path.read_text(encoding="utf-8"))
        if (
            isinstance(prev, dict)
            and prev.get("kind") == "genesis/cargo-target-dir-policy-v0.1"
            and isinstance(prev.get("violation_count"), int)
        ):
            doc["previous_violation_count"] = prev["violation_count"]
            doc["violation_delta"] = doc["violation_count"] - prev["violation_count"]
    except Exception:
        pass

report_path.parent.mkdir(parents=True, exist_ok=True)
history_path.parent.mkdir(parents=True, exist_ok=True)
report_path.write_text(json.dumps(doc, indent=2, sort_keys=True) + "\n", encoding="utf-8")
with history_path.open("a", encoding="utf-8") as fh:
    fh.write(json.dumps(doc, sort_keys=True) + "\n")

print(f"cargo-target-dir-policy: wrote report {report_path}")
if violations:
    print("cargo-target-dir-policy: missing policy helper in:", file=sys.stderr)
    for name in violations:
        print(f"  - {name}", file=sys.stderr)
    raise SystemExit(1)
print(
    "cargo-target-dir-policy: ok "
    f"(scanned={scanned}, cargo_scripts={cargo_scripts}, violations=0)"
)
PY
