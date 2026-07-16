#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

TOPOLOGY_FILE="docs/spec/DOC_TOPOLOGY_v0.1.md"
INDEX_FILE="docs/INDEX.md"
UPGRADE_PLAN_FILE="upgrade_plan.md"
FEATURE_MATRIX_FILE="feature_matrix.md"
REDTEAM_FILE="docs/status/REDTEAM_REPORT.md"

for required in \
  "$TOPOLOGY_FILE" \
  "$INDEX_FILE" \
  "$UPGRADE_PLAN_FILE" \
  "$FEATURE_MATRIX_FILE" \
  "$REDTEAM_FILE"
do
  [[ -f "$required" ]] || {
    echo "doc-topology-drift: missing required file: $required" >&2
    exit 1
  }
done

python3 - "$TOPOLOGY_FILE" "$INDEX_FILE" "$UPGRADE_PLAN_FILE" "$FEATURE_MATRIX_FILE" "$REDTEAM_FILE" <<'PY'
import pathlib
import re
import sys

topology_path = pathlib.Path(sys.argv[1])
index_path = pathlib.Path(sys.argv[2])
plan_path = pathlib.Path(sys.argv[3])
matrix_path = pathlib.Path(sys.argv[4])
redteam_path = pathlib.Path(sys.argv[5])

topology = topology_path.read_text(encoding="utf-8")
index = index_path.read_text(encoding="utf-8")
plan = plan_path.read_text(encoding="utf-8")
matrix = matrix_path.read_text(encoding="utf-8")
redteam = redteam_path.read_text(encoding="utf-8")

readiness_ref = ".genesis/perf/selfhost_readiness_report.json"
capability_ledger_ref = "docs/spec/CAPABILITY_EVIDENCE_LEDGER_v0.1.json"
if readiness_ref not in plan:
    raise SystemExit(
        "doc-topology-drift: upgrade_plan.md must reference "
        + readiness_ref
        + " as the machine-readable selfhost readiness source"
    )
if capability_ledger_ref not in matrix:
    raise SystemExit(
        "doc-topology-drift: feature_matrix.md must reference "
        + capability_ledger_ref
        + " as its canonical source"
    )
if "Aggregate Claim Boundary" not in matrix:
    raise SystemExit("doc-topology-drift: product target matrix must publish the aggregate claim boundary")

for header in (
    "## Authoring",
    "## Runtime",
    "## Assurance",
    "## Published Presentation",
    "## Operations",
    "## Update Workflow",
):
    if header not in topology:
        raise SystemExit(f"doc-topology-drift: topology missing required section: {header}")

for required_ref in (
    "docs/spec/AGENT_AUTHORING_BUNDLE_v0.1.md",
    "docs/spec/HOST_RUNTIME_BUNDLE_v0.1.md",
    "docs/spec/ASSURANCE_ARTIFACTS_v0.1.md",
    "docs/spec/WRITE_GENESISCODE_SKILL_CONFORMANCE_v0.1.md",
    "scripts/render_quarto_reference.py",
    "scripts/check_quarto_site.py",
    ".github/workflows/docs-site.yml",
    "docs/spec/DOC_COMPLEXITY_TARGETS_v0.1.md",
    "docs/spec/DOC_LEAF_OWNERSHIP_v0.1.md",
    "docs/spec/CAPABILITY_EVIDENCE_LEDGER_v0.1.json",
    "docs/status/SELFHOST_AUTHORITY_v0.1.md",
    "policies/check_update_boundary_v0.1.json",
    "docs/spec/CHECK_UPDATE_BOUNDARY_AUDIT_v0.1.json",
    "upgrade_plan.md",
    "feature_matrix.md",
    "docs/spec/PRODUCT_TARGET_MATRIX_v0.1.json",
    "docs/status/REDTEAM_REPORT.md",
):
    if required_ref not in topology:
        raise SystemExit(
            "doc-topology-drift: topology missing required canonical reference: "
            + required_ref
        )

if "docs/spec/DOC_TOPOLOGY_v0.1.md" not in index:
    raise SystemExit("doc-topology-drift: docs/INDEX.md must reference docs/spec/DOC_TOPOLOGY_v0.1.md")
if "docs/spec/DOC_COMPLEXITY_TARGETS_v0.1.md" not in index:
    raise SystemExit("doc-topology-drift: docs/INDEX.md must reference docs/spec/DOC_COMPLEXITY_TARGETS_v0.1.md")
if "docs/spec/DOC_LEAF_OWNERSHIP_v0.1.md" not in index:
    raise SystemExit("doc-topology-drift: docs/INDEX.md must reference docs/spec/DOC_LEAF_OWNERSHIP_v0.1.md")
for generated_status in (
    "docs/spec/CAPABILITY_EVIDENCE_LEDGER_v0.1.json",
    "docs/status/SELFHOST_AUTHORITY_v0.1.md",
    "docs/spec/PRODUCT_TARGET_MATRIX_v0.1.json",
    "policies/check_update_boundary_v0.1.json",
    "docs/spec/CHECK_UPDATE_BOUNDARY_AUDIT_v0.1.json",
):
    if generated_status not in index:
        raise SystemExit(
            "doc-topology-drift: docs/INDEX.md missing canonical/generated status reference: "
            + generated_status
        )

open_ids = sorted(
    set(
        re.findall(
            r"^- \[ \] (P\d+\.\d+)\b",
            plan,
            flags=re.MULTILINE,
        )
    )
)

lines = matrix.splitlines()
start = None
end = None
for i, line in enumerate(lines):
    if line.startswith("Known GenesisCode gaps"):
        start = i + 1
        continue
    if start is not None and line.startswith("Primary evidence paths"):
        end = i
        break
if start is None or end is None or end <= start:
    raise SystemExit(
        "doc-topology-drift: feature_matrix.md must contain 'Known GenesisCode gaps' section before 'Primary evidence paths'"
    )
matrix_ids = sorted(set(re.findall(r"\bP\d+\.\d+\b", "\n".join(lines[start:end]))))

missing_in_matrix = sorted(set(open_ids) - set(matrix_ids))
extra_in_matrix = sorted(set(matrix_ids) - set(open_ids))
if missing_in_matrix:
    raise SystemExit(
        "doc-topology-drift: feature_matrix known gaps missing open upgrade IDs: "
        + ", ".join(missing_in_matrix)
    )
if extra_in_matrix:
    raise SystemExit(
        "doc-topology-drift: feature_matrix known gaps include non-open IDs: "
        + ", ".join(extra_in_matrix)
    )

open_p0_p1 = sorted(
    set(
        re.findall(
            r"^- \[ \] (P[01]\.\d+)\b",
            plan,
            flags=re.MULTILINE,
        )
    )
)
redteam_ids = sorted(set(re.findall(r"\bP[01]\.\d+\b", redteam)))

missing_in_redteam = sorted(set(open_p0_p1) - set(redteam_ids))
extra_in_redteam = sorted(set(redteam_ids) - set(open_p0_p1))
if missing_in_redteam:
    raise SystemExit(
        "doc-topology-drift: redteam report missing open P0/P1 IDs: "
        + ", ".join(missing_in_redteam)
    )
if extra_in_redteam:
    raise SystemExit(
        "doc-topology-drift: redteam report contains stale P0/P1 IDs: "
        + ", ".join(extra_in_redteam)
    )

print(
    "doc-topology-drift: ok "
    f"(open_ids={len(open_ids)} matrix_ids={len(matrix_ids)} open_p0_p1={len(open_p0_p1)})"
)
PY

python3 scripts/render_quarto_reference.py --check
