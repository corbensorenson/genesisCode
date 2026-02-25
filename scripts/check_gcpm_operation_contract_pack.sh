#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

PKG_CONTRACT_FILE="crates/gc_cli_driver/src/pkg_contract.rs"
CLI_DRIVER_FILE="crates/gc_cli_driver/src/lib.rs"
SCHEMAS_DOC="docs/spec/GCPM_JSON_SCHEMAS_v0.1.md"
PACK_FILE="docs/spec/GCPM_OPERATION_CONTRACT_PACK_v0.1.json"
REPORT_OUT=".genesis/perf/gcpm_operation_contract_pack_report.json"

for required in "$PKG_CONTRACT_FILE" "$CLI_DRIVER_FILE" "$SCHEMAS_DOC" "$PACK_FILE"; do
  [[ -f "$required" ]] || {
    echo "gcpm-operation-contract-pack: missing required file: $required" >&2
    exit 1
  }
done

python3 - "$PKG_CONTRACT_FILE" "$CLI_DRIVER_FILE" "$SCHEMAS_DOC" "$PACK_FILE" "$REPORT_OUT" <<'PY'
import hashlib
import json
import pathlib
import re
import sys

pkg_contract_path = pathlib.Path(sys.argv[1])
cli_driver_path = pathlib.Path(sys.argv[2])
schemas_doc_path = pathlib.Path(sys.argv[3])
pack_path = pathlib.Path(sys.argv[4])
report_path = pathlib.Path(sys.argv[5])

pkg_contract_text = pkg_contract_path.read_text(encoding="utf-8")
cli_driver_text = cli_driver_path.read_text(encoding="utf-8")
schemas_doc = schemas_doc_path.read_text(encoding="utf-8")
pack_doc = json.loads(pack_path.read_text(encoding="utf-8"))

kind_section_split = pkg_contract_text.split("pub(crate) fn kind", 1)
if len(kind_section_split) != 2:
    raise SystemExit("gcpm-operation-contract-pack: unable to locate kind() section")
kind_section = kind_section_split[1].split("pub(crate) fn log_op", 1)[0]

log_section_split = pkg_contract_text.split("pub(crate) fn log_op", 1)
if len(log_section_split) != 2:
    raise SystemExit("gcpm-operation-contract-pack: unable to locate log_op() section")
log_section = log_section_split[1].split("#[cfg(test)]", 1)[0]

kind_matches = dict(
    re.findall(r'PkgCmd::([A-Za-z]+)\s*\{ \.\. \}\s*=>\s*"([^"]+)"', kind_section)
)

log_op_matches = dict(
    re.findall(r'PkgCmd::([A-Za-z]+)\s*\{ \.\. \}\s*=>\s*"([^"]+)"', log_section)
)

const_matches = {
    name: int(value)
    for name, value in re.findall(r"const\s+(EX_[A-Z_]+):\s*u8\s*=\s*(\d+);", cli_driver_text)
}

required_operations = {
    "new": "New",
    "scaffold": "Scaffold",
    "init": "Init",
    "add": "Add",
    "remove": "Remove",
    "lock": "Lock",
    "update": "Update",
    "run": "Run",
    "build": "Build",
    "test": "Test",
    "self-optimize": "SelfOptimize",
    "trace": "Trace",
    "qualify": "Qualify",
    "assurance-pack": "AssurancePack",
    "install": "Install",
    "verify": "Verify",
    "doctor": "Doctor",
    "list": "List",
    "info": "Info",
    "abi": "Abi",
    "snapshot": "Snapshot",
    "export": "Export",
    "import": "Import",
    "publish": "Publish",
    "bridge": "Bridge",
    "migrate": "Migrate",
    "env": "Env",
    "profile-runtime": "ProfileRuntime",
}

schema_doc_aliases = {
    "run": {"genesis/run-v0.2"},
    "test": {"genesis/test-v0.2"},
}

expected_failure_taxonomy = {
    "EX_PARSE": 10,
    "EX_EVAL": 20,
    "EX_OBLIGATIONS": 30,
    "EX_CAPS_DENIED": 41,
    "EX_VERIFY": 50,
    "EX_IO": 70,
}

errors: list[str] = []
operations: dict[str, dict[str, str]] = {}
for op_key, variant in required_operations.items():
    kind = kind_matches.get(variant)
    log_op = log_op_matches.get(variant)
    if not kind:
        errors.append(f"missing kind mapping for PkgCmd::{variant}")
        continue
    if not log_op:
        errors.append(f"missing log_op mapping for PkgCmd::{variant}")
        continue
    operations[op_key] = {"variant": variant, "kind": kind, "log_op": log_op}
    allowed_schema_ids = {kind} | schema_doc_aliases.get(op_key, set())
    if not any(schema_id in schemas_doc for schema_id in allowed_schema_ids):
        expected = ", ".join(sorted(allowed_schema_ids))
        errors.append(
            f"GCPM schema doc missing schema ID for {op_key}; expected one of: {expected}"
        )

expected_variants = set(required_operations.values())
kind_variants = set(kind_matches.keys())
log_variants = set(log_op_matches.keys())

missing_kind_variants = sorted(expected_variants - kind_variants)
missing_log_variants = sorted(expected_variants - log_variants)
extra_kind_variants = sorted(kind_variants - expected_variants)
extra_log_variants = sorted(log_variants - expected_variants)

if missing_kind_variants:
    errors.append("missing kind variants: " + ", ".join(missing_kind_variants))
if missing_log_variants:
    errors.append("missing log_op variants: " + ", ".join(missing_log_variants))
if extra_kind_variants:
    errors.append("unexpected kind variants: " + ", ".join(extra_kind_variants))
if extra_log_variants:
    errors.append("unexpected log_op variants: " + ", ".join(extra_log_variants))

failure_taxonomy: dict[str, int] = {}
for code_name, expected in expected_failure_taxonomy.items():
    actual = const_matches.get(code_name)
    if actual is None:
        errors.append(f"missing CLI exit code constant {code_name}")
        continue
    failure_taxonomy[code_name] = actual
    if actual != expected:
        errors.append(f"{code_name} expected {expected} got {actual}")

generated_pack = {
    "kind": "genesis/gcpm-operation-contract-pack-v0.1",
    "version": 1,
    "operations": operations,
    "failure_taxonomy": failure_taxonomy,
}

if pack_doc != generated_pack:
    errors.append(
        "contract pack JSON drift: docs/spec/GCPM_OPERATION_CONTRACT_PACK_v0.1.json "
        "must match extracted code contracts"
    )

pack_hash = hashlib.sha256(
    json.dumps(generated_pack, sort_keys=True, separators=(",", ":")).encode("utf-8")
).hexdigest()

report = {
    "kind": "genesis/gcpm-operation-contract-pack-report-v0.1",
    "ok": not errors,
    "contract_pack_path": pack_path.as_posix(),
    "pkg_contract_path": pkg_contract_path.as_posix(),
    "cli_driver_path": cli_driver_path.as_posix(),
    "schemas_doc_path": schemas_doc_path.as_posix(),
    "pack_hash": pack_hash,
    "operation_count": len(operations),
    "operations": operations,
    "failure_taxonomy": failure_taxonomy,
    "errors": errors,
}

report_path.parent.mkdir(parents=True, exist_ok=True)
report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")

if errors:
    raise SystemExit(
        "gcpm-operation-contract-pack: failed checks: " + "; ".join(errors)
    )

print(
    "gcpm-operation-contract-pack: ok "
    f"ops={len(operations)} pack_hash={pack_hash}"
)
PY
