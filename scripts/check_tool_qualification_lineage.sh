#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

REPORT_PATH="${GENESIS_TOOL_QUALIFICATION_LINEAGE_REPORT:-.genesis/perf/tool_qualification_lineage_report.json}"

python3 - "$ROOT_DIR" "$REPORT_PATH" <<'PY'
import json
import pathlib
import sys

root = pathlib.Path(sys.argv[1])
report_path = pathlib.Path(sys.argv[2])

checks = [
    (
        root / "crates/gc_cli_driver/src/cli_args/pkg_cmd.rs",
        "snapshot: String",
        "gcpm qualify must require --snapshot binding",
    ),
    (
        root / "crates/gc_cli_driver/src/pkg_assurance_ops.rs",
        "resolve_qualification_tests(",
        "gcpm qualify must resolve run-manifest lineage from store",
    ),
    (
        root / "crates/gc_cli_driver/src/pkg_assurance_ops_qualification.rs",
        "genesis/qualification-test-run-manifest-v0.1",
        "qualification lineage manifest kind contract must be defined",
    ),
    (
        root / "crates/gc_vcs/src/assurance.rs",
        ":manifest",
        "tool qualification gate must validate lineage manifest linkage fields",
    ),
    (
        root / "docs/spec/ASSURANCE_ARTIFACTS_v0.1.md",
        "run-manifest artifact hash",
        "assurance artifacts spec must document run-manifest linkage",
    ),
    (
        root / "docs/spec/CLI.md",
        "run-manifest-hex64",
        "CLI spec must document run-manifest based --test-artifact form",
    ),
]

missing = []
for file_path, needle, detail in checks:
    if not file_path.is_file():
        missing.append(f"missing file: {file_path.as_posix()}")
        continue
    text = file_path.read_text(encoding="utf-8")
    if needle not in text:
        missing.append(f"{detail} ({file_path.as_posix()} missing `{needle}`)")

ok = not missing
report = {
    "kind": "genesis/tool-qualification-lineage-report-v0.1",
    "ok": ok,
    "checked": [p.as_posix() for p, _, _ in checks],
    "errors": missing,
}
report_path.parent.mkdir(parents=True, exist_ok=True)
report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")

if not ok:
    print("tool-qualification-lineage: FAIL")
    for err in missing:
        print(f"  - {err}")
    raise SystemExit(1)

print(f"tool-qualification-lineage: ok (report={report_path.as_posix()})")
PY
