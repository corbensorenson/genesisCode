#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "vcs-selfhost-contract" \
  ".genesis/build/cargo" \
  "GENESIS_VCS_SELFHOST_CONTRACT_CARGO_TARGET_DIR"

REPORT_FILE="${GENESIS_VCS_SELFHOST_CONTRACT_REPORT:-.genesis/perf/vcs_selfhost_contract_report.json}"
CMD_VCS_FILE="crates/gc_cli_driver/src/cmd_vcs.rs"

python3 - "$ROOT_DIR" "$CMD_VCS_FILE" "$REPORT_FILE" <<'PY'
import json
import pathlib
import re
import subprocess
import sys

root = pathlib.Path(sys.argv[1]).resolve()
cmd_vcs_path = root / sys.argv[2]
report_path = root / sys.argv[3]

if not cmd_vcs_path.is_file():
    raise SystemExit(f"vcs-selfhost-contract: missing file: {cmd_vcs_path.relative_to(root).as_posix()}")

text = cmd_vcs_path.read_text(encoding="utf-8")
errors: list[str] = []

required_markers = [
    '#[cfg(feature = "parity-harness")]',
    '#[cfg(not(feature = "parity-harness"))]',
    "selfhost_program::build_selfhost_vcs_program",
]
missing_markers = [m for m in required_markers if m not in text]
if missing_markers:
    errors.append("missing-markers:" + " | ".join(missing_markers))

if not re.search(
    r'(?m)^[ \t]*#\[cfg\(feature = "parity-harness"\)\][ \t]*\n[ \t]*mod rust_program;',
    text,
):
    errors.append(
        "missing-cfg-gated-rust-program-module:mod rust_program must be immediately cfg-gated"
    )

if not re.search(
    r'(?ms)#\[cfg\(feature = "parity-harness"\)\][^\n]*\n[^\n]*let \(prog, program_hash\) = if frontend_is_rust',
    text,
):
    errors.append("missing-cfg-gated-rust-branch:frontend_is_rust branch must be parity-harness gated")

checks = []

def run_check(name: str, args: list[str]) -> None:
    proc = subprocess.run(
        args,
        cwd=root,
        capture_output=True,
        text=True,
    )
    checks.append(
        {
            "name": name,
            "args": args,
            "ok": proc.returncode == 0,
            "exit_code": proc.returncode,
            "stdout_tail": (proc.stdout or "").strip()[-500:],
            "stderr_tail": (proc.stderr or "").strip()[-500:],
        }
    )
    if proc.returncode != 0:
        errors.append(f"compile-check-failed:{name}:exit={proc.returncode}")

run_check(
    "gc_cli_driver_production",
    [
        "cargo",
        "check",
        "-p",
        "gc_cli_driver",
        "--no-default-features",
        "--features",
        "profile-headless",
    ],
)
run_check("gc_cli_driver_parity", ["cargo", "check", "-p", "gc_cli_driver_parity"])

report = {
    "kind": "genesis/vcs-selfhost-contract-v0.1",
    "ok": not errors,
    "errors": errors,
    "cmd_vcs_file": cmd_vcs_path.relative_to(root).as_posix(),
    "missing_markers": missing_markers,
    "checks": checks,
}
report_path.parent.mkdir(parents=True, exist_ok=True)
report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")

if errors:
    raise SystemExit("vcs-selfhost-contract: " + " | ".join(errors))

print(
    "vcs-selfhost-contract: ok "
    f"(checks={len(checks)} report={report_path.relative_to(root).as_posix()})"
)
PY
