#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

DOC_PATH="docs/spec/FULL_SELFHOST_CUTOVER_PROFILE_v0.1.md"
REPORT_PATH="${GENESIS_FULL_SELFHOST_CUTOVER_REPORT:-.genesis/perf/full_selfhost_cutover_profile_report.json}"
HISTORY_PATH="${GENESIS_FULL_SELFHOST_CUTOVER_HISTORY:-.genesis/perf/full_selfhost_cutover_profile_history.jsonl}"
REFRESH="${GENESIS_FULL_SELFHOST_CUTOVER_REFRESH:-0}"
READINESS_REPORT="${GENESIS_SELFHOST_READINESS_REPORT:-.genesis/perf/selfhost_readiness_report.json}"
BOOTSTRAP_REPORT="${GENESIS_BOOTSTRAP_RETIREMENT_REPORT:-.genesis/perf/bootstrap_retirement_gate_report.json}"
DASHBOARD_FRESH_REPORT="${GENESIS_SELFHOST_DASHBOARD_FRESH_REPORT:-.genesis/perf/selfhost_dashboard_fresh_report.json}"
KERNEL_TCB_REPORT="${GENESIS_KERNEL_TCB_REPORT:-.genesis/perf/kernel_tcb_contract_report.json}"
HOST_API_EVOLUTION_REPORT="${GENESIS_HOST_API_EVOLUTION_REPORT:-.genesis/perf/host_api_evolution_contract_report.json}"
GCPM_CONTRACT_PACK_REPORT="${GENESIS_GCPM_OPERATION_CONTRACT_PACK_REPORT:-.genesis/perf/gcpm_operation_contract_pack_report.json}"
VCS_SELFHOST_REPORT="${GENESIS_VCS_SELFHOST_CONTRACT_REPORT:-.genesis/perf/vcs_selfhost_contract_report.json}"
SELFHOST_SYMBOL_OWNERSHIP_REPORT="${GENESIS_SELFHOST_SYMBOL_OWNERSHIP_REPORT:-.genesis/perf/selfhost_symbol_ownership_report.json}"
SELFHOST_ARTIFACT="${GENESIS_SELFHOST_TOOLCHAIN_ARTIFACT:-selfhost/toolchain.gc}"
AGENT_GENERATIVE_REPORT="${GENESIS_AGENT_GENERATIVE_REPORT:-.genesis/perf/agent_generative_workloads_report.json}"

if [[ "$REFRESH" != "0" && "$REFRESH" != "1" ]]; then
  echo "full-selfhost-cutover-profile: GENESIS_FULL_SELFHOST_CUTOVER_REFRESH must be 0 or 1" >&2
  exit 2
fi

if [[ "$REFRESH" == "1" ]]; then
  bash scripts/check_selfhost_boundary.sh --strict
  bash scripts/check_bootstrap_retirement_gate.sh
  bash scripts/check_selfhost_dashboard_fresh.sh
  bash scripts/check_selfhost_readiness_scorecard.sh
  bash scripts/check_kernel_tcb_contract.sh
  bash scripts/check_host_api_evolution_contracts.sh
  bash scripts/check_gcpm_operation_contract_pack.sh
  bash scripts/check_vcs_selfhost_contract.sh
  bash scripts/check_selfhost_symbol_ownership.sh
fi

[[ -f "$DOC_PATH" ]] || {
  echo "full-selfhost-cutover-profile: missing doc: $DOC_PATH" >&2
  exit 1
}
[[ -f "$READINESS_REPORT" ]] || {
  echo "full-selfhost-cutover-profile: missing readiness report: $READINESS_REPORT" >&2
  exit 1
}
[[ -f "$BOOTSTRAP_REPORT" ]] || {
  echo "full-selfhost-cutover-profile: missing bootstrap retirement report: $BOOTSTRAP_REPORT" >&2
  exit 1
}
[[ -f "$DASHBOARD_FRESH_REPORT" ]] || {
  echo "full-selfhost-cutover-profile: missing selfhost dashboard freshness report: $DASHBOARD_FRESH_REPORT" >&2
  exit 1
}
[[ -f "$KERNEL_TCB_REPORT" ]] || {
  echo "full-selfhost-cutover-profile: missing kernel tcb report: $KERNEL_TCB_REPORT" >&2
  exit 1
}
[[ -f "$HOST_API_EVOLUTION_REPORT" ]] || {
  echo "full-selfhost-cutover-profile: missing host api evolution report: $HOST_API_EVOLUTION_REPORT" >&2
  exit 1
}
[[ -f "$GCPM_CONTRACT_PACK_REPORT" ]] || {
  echo "full-selfhost-cutover-profile: missing gcpm operation contract pack report: $GCPM_CONTRACT_PACK_REPORT" >&2
  exit 1
}
[[ -f "$VCS_SELFHOST_REPORT" ]] || {
  echo "full-selfhost-cutover-profile: missing vcs selfhost contract report: $VCS_SELFHOST_REPORT" >&2
  exit 1
}
[[ -f "$SELFHOST_SYMBOL_OWNERSHIP_REPORT" ]] || {
  echo "full-selfhost-cutover-profile: missing selfhost symbol ownership report: $SELFHOST_SYMBOL_OWNERSHIP_REPORT" >&2
  exit 1
}
[[ -f "$SELFHOST_ARTIFACT" ]] || {
  echo "full-selfhost-cutover-profile: missing selfhost toolchain artifact: $SELFHOST_ARTIFACT" >&2
  exit 1
}
[[ -f "$AGENT_GENERATIVE_REPORT" ]] || {
  echo "full-selfhost-cutover-profile: missing agent generative workloads report: $AGENT_GENERATIVE_REPORT" >&2
  exit 1
}

python3 - "$ROOT_DIR" "$DOC_PATH" "$READINESS_REPORT" "$BOOTSTRAP_REPORT" "$DASHBOARD_FRESH_REPORT" "$KERNEL_TCB_REPORT" "$HOST_API_EVOLUTION_REPORT" "$GCPM_CONTRACT_PACK_REPORT" "$VCS_SELFHOST_REPORT" "$SELFHOST_SYMBOL_OWNERSHIP_REPORT" "$SELFHOST_ARTIFACT" "$AGENT_GENERATIVE_REPORT" "$REPORT_PATH" "$HISTORY_PATH" <<'PY'
import datetime as dt
import json
import pathlib
import re
import sys

root = pathlib.Path(sys.argv[1]).resolve()
doc_path = root / sys.argv[2]
readiness_path = root / sys.argv[3]
bootstrap_path = root / sys.argv[4]
dashboard_path = root / sys.argv[5]
kernel_tcb_path = root / sys.argv[6]
host_api_path = root / sys.argv[7]
gcpm_contract_path = root / sys.argv[8]
vcs_selfhost_path = root / sys.argv[9]
selfhost_symbol_path = root / sys.argv[10]
selfhost_artifact_path = root / sys.argv[11]
agent_generative_path = root / sys.argv[12]
report_path = root / sys.argv[13]
history_path = root / sys.argv[14]

doc = doc_path.read_text(encoding="utf-8")

required_headings = [
    "# Full-Selfhost Cutover Profile v0.1",
    "## Remaining Exceptions (Explicit)",
    "## Exception Ownership + No-Semantic-Drift Proofs",
    "## Closure Path",
    "## Gate Contract",
]
missing_headings = [h for h in required_headings if h not in doc]
if missing_headings:
    raise SystemExit(
        "full-selfhost-cutover-profile: missing required heading(s): "
        + ", ".join(missing_headings)
    )

exception_section_match = re.search(
    r"## Remaining Exceptions \(Explicit\)\s*(.*?)(?:\n## |\Z)",
    doc,
    flags=re.DOTALL,
)
if exception_section_match is None:
    raise SystemExit(
        "full-selfhost-cutover-profile: explicit exception section missing"
    )
exception_section = exception_section_match.group(1)
explicit_exception_rows = re.findall(
    r"^\s*-\s+`([^`]+)`\s*$",
    exception_section,
    flags=re.MULTILINE,
)
if explicit_exception_rows:
    raise SystemExit(
        "full-selfhost-cutover-profile: explicit exception carve-outs are not allowed: "
        + ", ".join(sorted(set(explicit_exception_rows)))
    )
if re.search(r"^\s*-\s+none\b", exception_section, flags=re.IGNORECASE | re.MULTILINE) is None:
    raise SystemExit(
        "full-selfhost-cutover-profile: explicit exception section must declare `- none`"
    )

if "scripts/check_full_selfhost_cutover_profile.sh" not in doc:
    raise SystemExit(
        "full-selfhost-cutover-profile: gate contract must reference scripts/check_full_selfhost_cutover_profile.sh"
    )
if "scripts/check_selfhost_boundary.sh --strict" not in doc:
    raise SystemExit(
        "full-selfhost-cutover-profile: gate contract must reference strict selfhost boundary guard"
    )

readiness = json.loads(readiness_path.read_text(encoding="utf-8"))
if readiness.get("kind") != "genesis/selfhost-readiness-v0.1":
    raise SystemExit(
        "full-selfhost-cutover-profile: readiness report kind mismatch"
    )

dimensions = readiness.get("dimensions")
if not isinstance(dimensions, dict) or not dimensions:
    raise SystemExit(
        "full-selfhost-cutover-profile: readiness report dimensions missing/invalid"
    )
dimension_failures = sorted(
    name for name, spec in dimensions.items() if not bool(spec.get("ok", False))
)
if dimension_failures:
    raise SystemExit(
        "full-selfhost-cutover-profile: readiness dimension(s) failing: "
        + ", ".join(dimension_failures)
    )

fail_reasons = readiness.get("fail_reasons", [])
if not isinstance(fail_reasons, list):
    raise SystemExit(
        "full-selfhost-cutover-profile: readiness fail_reasons must be a list"
    )
invalid_fail_reasons = sorted(
    {str(x) for x in fail_reasons if str(x) != "open-upgrade-plan-ids"}
)
if invalid_fail_reasons:
    raise SystemExit(
        "full-selfhost-cutover-profile: unsupported readiness fail reasons: "
        + ", ".join(invalid_fail_reasons)
    )

bootstrap = json.loads(bootstrap_path.read_text(encoding="utf-8"))
if bootstrap.get("kind") != "genesis/bootstrap-retirement-gate-report-v0.1":
    raise SystemExit(
        "full-selfhost-cutover-profile: bootstrap retirement report kind mismatch"
    )
bootstrap_status = str(bootstrap.get("status", ""))
if bootstrap_status not in {"ok", "degraded"}:
    raise SystemExit(
        "full-selfhost-cutover-profile: bootstrap retirement status must be ok|degraded"
    )

dashboard = json.loads(dashboard_path.read_text(encoding="utf-8"))
if dashboard.get("kind") != "genesis/selfhost-dashboard-fresh-v0.1":
    raise SystemExit(
        "full-selfhost-cutover-profile: dashboard freshness report kind mismatch"
    )
if not bool(dashboard.get("ok", False)):
    raise SystemExit(
        "full-selfhost-cutover-profile: dashboard freshness report is not ok"
    )

agent_generative = json.loads(agent_generative_path.read_text(encoding="utf-8"))
if agent_generative.get("kind") != "genesis/agent-generative-workloads-v0.1":
    raise SystemExit(
        "full-selfhost-cutover-profile: agent generative workloads report kind mismatch"
    )
if not bool(agent_generative.get("ok", False)):
    raise SystemExit(
        "full-selfhost-cutover-profile: agent generative workloads report is not ok"
    )
required_domains = agent_generative.get("required_domains")
if not isinstance(required_domains, list) or not required_domains:
    raise SystemExit(
        "full-selfhost-cutover-profile: agent generative workloads required_domains missing/invalid"
    )
if not all(isinstance(d, str) and d for d in required_domains):
    raise SystemExit(
        "full-selfhost-cutover-profile: agent generative workloads required_domains must be non-empty strings"
    )
generative_domain_count = len(set(required_domains))
# Stage2 minima must scale with the active generative corpus surface. The +2 margin
# reserves headroom for non-domain utility modules in the selfhost toolchain.
generative_stage2_minimum = generative_domain_count + 2

artifact_text = selfhost_artifact_path.read_text(encoding="utf-8")

def parse_stage2_section(section_label: str) -> str:
    match = re.search(
        rf"{re.escape(section_label)}\s*\{{([^{{}}]*)\}}",
        artifact_text,
        flags=re.DOTALL,
    )
    if match is None:
        raise SystemExit(
            f"full-selfhost-cutover-profile: selfhost artifact missing section {section_label}: {selfhost_artifact_path}"
        )
    return match.group(1)

def parse_stage2_int(section: str, key: str) -> int:
    match = re.search(rf"{re.escape(key)}\s+(-?\d+)\b", section)
    if match is None:
        raise SystemExit(
            f"full-selfhost-cutover-profile: selfhost artifact missing integer field {key}"
        )
    return int(match.group(1))

summary_section = parse_stage2_section(":stage2-summary")
requirements_section = parse_stage2_section(":stage2-requirements")
stage2_supported_modules = parse_stage2_int(summary_section, ":supported-modules")
stage2_validated_modules = parse_stage2_int(summary_section, ":validated-modules")
stage2_min_supported_modules = parse_stage2_int(requirements_section, ":min-supported-modules")
stage2_min_validated_modules = parse_stage2_int(requirements_section, ":min-validated-modules")
requirements_ok = bool(
    re.search(r":ok\s+true(?:\b|$)", requirements_section)
)
if stage2_min_supported_modules <= 0 or stage2_min_validated_modules <= 0:
    raise SystemExit(
        "full-selfhost-cutover-profile: stage2 requirement minima must be non-zero in selfhost artifact"
    )
if stage2_supported_modules < stage2_min_supported_modules:
    raise SystemExit(
        "full-selfhost-cutover-profile: stage2 supported modules below enforced minimum "
        f"({stage2_supported_modules} < {stage2_min_supported_modules})"
    )
if stage2_validated_modules < stage2_min_validated_modules:
    raise SystemExit(
        "full-selfhost-cutover-profile: stage2 validated modules below enforced minimum "
        f"({stage2_validated_modules} < {stage2_min_validated_modules})"
    )
if not requirements_ok:
    raise SystemExit(
        "full-selfhost-cutover-profile: selfhost artifact stage2 requirements are not marked ok"
    )
if stage2_min_supported_modules < generative_stage2_minimum:
    raise SystemExit(
        "full-selfhost-cutover-profile: stage2 supported minimum is below generative corpus floor "
        f"({stage2_min_supported_modules} < {generative_stage2_minimum})"
    )
if stage2_min_validated_modules < generative_stage2_minimum:
    raise SystemExit(
        "full-selfhost-cutover-profile: stage2 validated minimum is below generative corpus floor "
        f"({stage2_min_validated_modules} < {generative_stage2_minimum})"
    )

proof_specs = {
    "kernel_tcb_contract": (
        kernel_tcb_path,
        "genesis/kernel-tcb-contract-v0.1",
    ),
    "host_api_evolution_contract": (
        host_api_path,
        "genesis/host-api-evolution-contract-report-v0.1",
    ),
    "gcpm_operation_contract_pack": (
        gcpm_contract_path,
        "genesis/gcpm-operation-contract-pack-report-v0.1",
    ),
    "vcs_selfhost_contract": (
        vcs_selfhost_path,
        "genesis/vcs-selfhost-contract-v0.1",
    ),
    "selfhost_symbol_ownership": (
        selfhost_symbol_path,
        "genesis/selfhost-symbol-ownership-v0.1",
    ),
}

proof_reports = {}
for name, (path, expected_kind) in proof_specs.items():
    if not path.is_file():
        raise SystemExit(
            f"full-selfhost-cutover-profile: required proof report missing for {name}: {path}"
        )
    try:
        proof_doc = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        raise SystemExit(
            f"full-selfhost-cutover-profile: proof report {name} is invalid JSON: {path}"
        ) from exc
    if proof_doc.get("kind") != expected_kind:
        raise SystemExit(
            "full-selfhost-cutover-profile: proof report kind mismatch for "
            f"{name}: expected {expected_kind!r}, got {proof_doc.get('kind')!r}"
        )
    if not bool(proof_doc.get("ok", False)):
        raise SystemExit(
            f"full-selfhost-cutover-profile: proof report {name} is not ok: {path}"
        )
    proof_reports[name] = path.relative_to(root).as_posix()

timestamp_utc = dt.datetime.now(dt.timezone.utc).replace(microsecond=0).isoformat()
report_doc = {
    "kind": "genesis/full-selfhost-cutover-profile-v0.1",
    "timestamp_utc": timestamp_utc,
    "doc": doc_path.relative_to(root).as_posix(),
    "readiness_report": readiness_path.relative_to(root).as_posix(),
    "bootstrap_report": bootstrap_path.relative_to(root).as_posix(),
    "dashboard_fresh_report": dashboard_path.relative_to(root).as_posix(),
    "selfhost_artifact": selfhost_artifact_path.relative_to(root).as_posix(),
    "agent_generative_report": agent_generative_path.relative_to(root).as_posix(),
    "explicit_exceptions": [],
    "readiness_dimension_count": len(dimensions),
    "readiness_fail_reasons": [str(x) for x in fail_reasons],
    "bootstrap_status": bootstrap_status,
    "stage2_supported_modules": stage2_supported_modules,
    "stage2_validated_modules": stage2_validated_modules,
    "stage2_min_supported_modules": stage2_min_supported_modules,
    "stage2_min_validated_modules": stage2_min_validated_modules,
    "stage2_generative_domain_count": generative_domain_count,
    "stage2_generative_minimum": generative_stage2_minimum,
    "stage2_requirements_ok": requirements_ok,
    "exception_proof_reports": proof_reports,
    "ok": True,
}
report_path.parent.mkdir(parents=True, exist_ok=True)
report_path.write_text(json.dumps(report_doc, indent=2, sort_keys=True) + "\n", encoding="utf-8")
history_path.parent.mkdir(parents=True, exist_ok=True)
with history_path.open("a", encoding="utf-8") as fh:
    fh.write(
        json.dumps(
            {
                "kind": report_doc["kind"],
                "timestamp_utc": timestamp_utc,
                "ok": True,
                "stage2_supported_modules": stage2_supported_modules,
                "stage2_validated_modules": stage2_validated_modules,
                "stage2_min_supported_modules": stage2_min_supported_modules,
                "stage2_min_validated_modules": stage2_min_validated_modules,
                "stage2_generative_domain_count": generative_domain_count,
                "stage2_generative_minimum": generative_stage2_minimum,
                "stage2_requirements_ok": requirements_ok,
                "report": report_path.relative_to(root).as_posix(),
            },
            sort_keys=True,
        )
        + "\n"
    )
print(
    "full-selfhost-cutover-profile: ok "
    f"(dimensions={len(dimensions)} bootstrap_status={bootstrap_status} stage2_min_supported={stage2_min_supported_modules} stage2_min_validated={stage2_min_validated_modules} generative_floor={generative_stage2_minimum})"
)
PY
