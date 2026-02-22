#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

MATRIX_PATH="${GENESIS_FEATURE_MATRIX_PATH:-feature_matrix.md}"
LEDGER_JSON="${GENESIS_FEATURE_MATRIX_EVIDENCE_JSON:-docs/spec/FEATURE_MATRIX_EVIDENCE_v0.1.json}"
LEDGER_MD="${GENESIS_FEATURE_MATRIX_EVIDENCE_MD:-docs/spec/FEATURE_MATRIX_EVIDENCE_v0.1.md}"

[[ -f "$MATRIX_PATH" ]] || {
  echo "feature-matrix-evidence: missing feature matrix file: $MATRIX_PATH" >&2
  exit 1
}
[[ -f "$LEDGER_JSON" ]] || {
  echo "feature-matrix-evidence: missing ledger json file: $LEDGER_JSON" >&2
  echo "feature-matrix-evidence: run scripts/update_feature_matrix_evidence.sh" >&2
  exit 1
}
[[ -f "$LEDGER_MD" ]] || {
  echo "feature-matrix-evidence: missing ledger markdown file: $LEDGER_MD" >&2
  echo "feature-matrix-evidence: run scripts/update_feature_matrix_evidence.sh" >&2
  exit 1
}

python3 - "$ROOT_DIR" "$MATRIX_PATH" "$LEDGER_JSON" "$LEDGER_MD" <<'PY'
import json
import pathlib
import re
import sys

root = pathlib.Path(sys.argv[1])
matrix = root / sys.argv[2]
ledger_json_path = root / sys.argv[3]
ledger_md_path = root / sys.argv[4]

lines = matrix.read_text(encoding="utf-8").splitlines()
start = None
for i, line in enumerate(lines):
    if line.startswith("| Capability |"):
        start = i + 2
        break
if start is None:
    raise SystemExit("feature-matrix-evidence: feature matrix table header not found")

capabilities = []
for line in lines[start:]:
    if not line.startswith("|"):
        break
    cols = [c.strip() for c in line.strip().strip("|").split("|")]
    if not cols or not cols[0]:
        continue
    if cols[0] not in capabilities:
        capabilities.append(cols[0])

ledger = json.loads(ledger_json_path.read_text(encoding="utf-8"))
if ledger.get("kind") != "genesis/feature-matrix-evidence-v0.1":
    raise SystemExit(
        "feature-matrix-evidence: invalid ledger kind in docs/spec/FEATURE_MATRIX_EVIDENCE_v0.1.json"
    )

entries = ledger.get("entries")
if not isinstance(entries, list) or not entries:
    raise SystemExit("feature-matrix-evidence: ledger entries must be a non-empty list")

entry_map = {}
for entry in entries:
    if not isinstance(entry, dict):
        raise SystemExit("feature-matrix-evidence: every ledger entry must be an object")
    cap = entry.get("capability")
    ev = entry.get("evidence_paths")
    ck = entry.get("check_paths")
    if not cap or not isinstance(cap, str):
        raise SystemExit("feature-matrix-evidence: every entry must include capability string")
    if cap in entry_map:
        raise SystemExit(f"feature-matrix-evidence: duplicate capability entry: {cap}")
    if not isinstance(ev, list) or not ev:
        raise SystemExit(f"feature-matrix-evidence: capability has empty evidence_paths: {cap}")
    if not isinstance(ck, list) or not ck:
        raise SystemExit(f"feature-matrix-evidence: capability has empty check_paths: {cap}")
    for p in ev + ck:
        if not isinstance(p, str):
            raise SystemExit(f"feature-matrix-evidence: non-string path in capability: {cap}")
        if not (root / p).exists():
            raise SystemExit(
                f"feature-matrix-evidence: path does not exist for capability '{cap}': {p}"
            )
    entry_map[cap] = entry

required_claim_mappings = {
    "GPU compute + graphics capability surfaces": {
        "evidence_paths": [
            "docs/spec/GPU_COMPUTE_BUNDLE_v0.1.md",
            "docs/spec/GFX_RUNTIME_BUNDLE_v0.1.md",
        ],
        "check_paths": [
            "scripts/check_gpu_compute_runtime_profile.sh",
            "scripts/check_gfx_runtime_profile.sh",
            "scripts/check_gpu_stack_decoupling.sh",
        ],
    },
    "Deployment/bundle target pipeline in core toolchain": {
        "evidence_paths": [
            "docs/spec/GCPM_JSON_SCHEMAS_v0.1.md",
            "docs/spec/GCPM_WORKFLOW_REPORTS_v0.1.md",
        ],
        "check_paths": [
            "crates/gc_cli/tests/cli_pkg_workspace.rs",
            "examples/agent_deploy_bundle_workflow/workflow.sh",
        ],
    },
}

for capability, req in required_claim_mappings.items():
    entry = entry_map.get(capability)
    if entry is None:
        raise SystemExit(
            f"feature-matrix-evidence: required capability missing from ledger: {capability}"
        )
    ev_set = set(entry.get("evidence_paths", []))
    ck_set = set(entry.get("check_paths", []))
    missing_ev = [p for p in req["evidence_paths"] if p not in ev_set]
    missing_ck = [p for p in req["check_paths"] if p not in ck_set]
    if missing_ev:
        raise SystemExit(
            "feature-matrix-evidence: capability missing required evidence mapping(s): "
            f"{capability}: " + ", ".join(missing_ev)
        )
    if missing_ck:
        raise SystemExit(
            "feature-matrix-evidence: capability missing required check mapping(s): "
            f"{capability}: " + ", ".join(missing_ck)
        )

missing = [cap for cap in capabilities if cap not in entry_map]
extra = [cap for cap in entry_map if cap not in capabilities]
if missing:
    raise SystemExit(
        "feature-matrix-evidence: missing capability entries for: " + ", ".join(missing)
    )
if extra:
    raise SystemExit(
        "feature-matrix-evidence: stale capability entries not in matrix: " + ", ".join(extra)
    )

md = ledger_md_path.read_text(encoding="utf-8")
if "Feature Matrix Evidence Ledger v0.1" not in md:
    raise SystemExit(
        "feature-matrix-evidence: markdown ledger title missing in docs/spec/FEATURE_MATRIX_EVIDENCE_v0.1.md"
    )
if "docs/spec/FEATURE_MATRIX_EVIDENCE_v0.1.json" not in md and "feature_matrix.md" not in md:
    raise SystemExit(
        "feature-matrix-evidence: markdown ledger must reference source/contract artifacts"
    )

print(
    f"feature-matrix-evidence: ok (capabilities={len(capabilities)} entries={len(entries)})"
)
PY
