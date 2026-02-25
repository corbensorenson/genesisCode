#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

INDEX_FILE="docs/spec/HOST_ABI_SCHEMA_INDEX_v0.1.json"
SPEC_FILE="docs/spec/HOST_ABI.md"
REPORT_OUT=".genesis/perf/host_api_evolution_contract_report.json"

[[ -f "$INDEX_FILE" ]] || {
  echo "host-api-evolution-contracts: missing schema index: $INDEX_FILE" >&2
  exit 1
}
[[ -f "$SPEC_FILE" ]] || {
  echo "host-api-evolution-contracts: missing spec doc: $SPEC_FILE" >&2
  exit 1
}

python3 - "$INDEX_FILE" "$SPEC_FILE" "$REPORT_OUT" <<'PY'
import hashlib
import json
import pathlib
import sys
from dataclasses import dataclass

index_path = pathlib.Path(sys.argv[1])
spec_path = pathlib.Path(sys.argv[2])
report_path = pathlib.Path(sys.argv[3])

doc = json.loads(index_path.read_text(encoding="utf-8"))
if doc.get("kind") != "genesis/host-abi-schema-index-v0.1":
    raise SystemExit(
        "host-api-evolution-contracts: unexpected schema index kind; "
        "expected genesis/host-abi-schema-index-v0.1"
    )

ops = doc.get("operations")
if not isinstance(ops, dict) or not ops:
    raise SystemExit("host-api-evolution-contracts: operations map is missing or empty")

spec_text = spec_path.read_text(encoding="utf-8")
for marker in (
    "### High-Churn Host API Evolution Contract",
    "#### Versioned Contract Families",
    "#### Deterministic Evolution Rules",
    "#### Machine Checks",
):
    if marker not in spec_text:
        raise SystemExit(
            "host-api-evolution-contracts: spec doc missing required section "
            + marker
        )


@dataclass(frozen=True)
class DomainContract:
    name: str
    prefixes: tuple[str, ...]
    min_ops: int


domain_contracts = (
    DomainContract("gpu-compute", ("gpu/compute::",), 10),
    DomainContract("gfx-gpu", ("gfx/gpu::",), 10),
    DomainContract("xr", ("gfx/xr::",), 10),
    DomainContract(
        "editor",
        (
            "editor/task::",
            "editor/watch::",
            "editor/dialog::",
            "editor/clipboard::",
            "editor/plugin::",
        ),
        10,
    ),
    DomainContract("network", ("io/net::",), 12),
    DomainContract("ffi", ("host/ffi::",), 3),
    DomainContract("plugin", ("host/plugin::", "editor/plugin::"), 2),
)

errors: list[str] = []
domain_summaries: dict[str, dict[str, object]] = {}
required_plugin_ops = {"host/plugin::command", "editor/plugin::command"}
seen_plugin_ops: set[str] = set()
required_ffi_ops = {"host/ffi::call", "host/ffi::buffer-pin", "host/ffi::buffer-unpin"}
seen_ffi_ops: set[str] = set()

for domain in domain_contracts:
    matched_ops = sorted(
        op
        for op in ops.keys()
        if any(op.startswith(prefix) for prefix in domain.prefixes)
    )
    if len(matched_ops) < domain.min_ops:
        errors.append(
            f"{domain.name}: expected at least {domain.min_ops} ops, found {len(matched_ops)}"
        )
    op_fingerprints: list[str] = []
    for op in matched_ops:
        entry = ops.get(op)
        if not isinstance(entry, dict):
            errors.append(f"{domain.name}:{op}: entry must be object")
            continue
        payload = entry.get("payload")
        response = entry.get("response_envelope")
        if not isinstance(payload, dict):
            errors.append(f"{domain.name}:{op}: payload contract missing")
            continue
        if not isinstance(payload.get("type"), str) or not payload.get("type"):
            errors.append(f"{domain.name}:{op}: payload.type missing")
        if not isinstance(response, dict):
            errors.append(f"{domain.name}:{op}: response_envelope missing")
            continue
        success = response.get("success")
        error = response.get("error")
        if not isinstance(success, dict):
            errors.append(f"{domain.name}:{op}: success envelope missing")
        else:
            kind = success.get("value_kind")
            if not isinstance(kind, str) or not kind:
                errors.append(f"{domain.name}:{op}: success.value_kind missing")
        if not isinstance(error, dict):
            errors.append(f"{domain.name}:{op}: error envelope missing")
        else:
            if error.get("sealed") is not True:
                errors.append(f"{domain.name}:{op}: error.sealed must be true")
            code_prefix = error.get("code_prefix")
            if not isinstance(code_prefix, str) or not code_prefix.startswith("core/caps/"):
                errors.append(
                    f"{domain.name}:{op}: error.code_prefix must start with core/caps/"
                )
        canonical = json.dumps(entry, sort_keys=True, separators=(",", ":"))
        op_hash = hashlib.sha256(canonical.encode("utf-8")).hexdigest()
        op_fingerprints.append(f"{op}:{op_hash}")
        if op in required_plugin_ops:
            seen_plugin_ops.add(op)
        if op in required_ffi_ops:
            seen_ffi_ops.add(op)

    domain_hash = hashlib.sha256(
        "\n".join(op_fingerprints).encode("utf-8")
    ).hexdigest()
    domain_summaries[domain.name] = {
        "prefixes": list(domain.prefixes),
        "op_count": len(matched_ops),
        "ops": matched_ops,
        "domain_hash": domain_hash,
    }

missing_plugin_ops = sorted(required_plugin_ops - seen_plugin_ops)
if missing_plugin_ops:
    errors.append(
        "plugin: missing required plugin evolution op(s): " + ", ".join(missing_plugin_ops)
    )

missing_ffi_ops = sorted(required_ffi_ops - seen_ffi_ops)
if missing_ffi_ops:
    errors.append(
        "ffi: missing required ffi evolution op(s): " + ", ".join(missing_ffi_ops)
    )

overall_hash = hashlib.sha256(
    json.dumps(domain_summaries, sort_keys=True, separators=(",", ":")).encode("utf-8")
).hexdigest()

report = {
    "kind": "genesis/host-api-evolution-contract-report-v0.1",
    "ok": not errors,
    "schema_index_path": index_path.as_posix(),
    "spec_path": spec_path.as_posix(),
    "domain_count": len(domain_contracts),
    "domains": domain_summaries,
    "overall_contract_hash": overall_hash,
    "errors": errors,
}

report_path.parent.mkdir(parents=True, exist_ok=True)
report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")

if errors:
    raise SystemExit(
        "host-api-evolution-contracts: failed checks: " + "; ".join(errors)
    )

print(
    "host-api-evolution-contracts: ok "
    f"domains={len(domain_contracts)} overall_hash={overall_hash}"
)
PY
