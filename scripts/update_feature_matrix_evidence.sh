#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

MATRIX_PATH="${GENESIS_FEATURE_MATRIX_PATH:-feature_matrix.md}"
OUT_JSON="${GENESIS_FEATURE_MATRIX_EVIDENCE_JSON:-docs/spec/FEATURE_MATRIX_EVIDENCE_v0.1.json}"
OUT_MD="${GENESIS_FEATURE_MATRIX_EVIDENCE_MD:-docs/spec/FEATURE_MATRIX_EVIDENCE_v0.1.md}"

python3 - "$ROOT_DIR" "$MATRIX_PATH" "$OUT_JSON" "$OUT_MD" <<'PY'
import json
import pathlib
import re
import sys

root = pathlib.Path(sys.argv[1])
matrix_path = root / sys.argv[2]
out_json = root / sys.argv[3]
out_md = root / sys.argv[4]

if not matrix_path.is_file():
    raise SystemExit(f"update-feature-matrix-evidence: missing feature matrix: {matrix_path}")

lines = matrix_path.read_text(encoding="utf-8").splitlines()
start = None
for i, line in enumerate(lines):
    if line.startswith("| Capability |"):
        start = i + 2
        break
if start is None:
    raise SystemExit("update-feature-matrix-evidence: capability table header missing")

capabilities = []
for line in lines[start:]:
    if not line.startswith("|"):
        break
    cols = [c.strip() for c in line.strip().strip("|").split("|")]
    if len(cols) < 2:
        continue
    capability = cols[0]
    if capability and capability not in capabilities:
        capabilities.append(capability)

if not capabilities:
    raise SystemExit("update-feature-matrix-evidence: no capabilities discovered in matrix table")

def classify(cap: str):
    cap_l = cap.lower()
    evidence = []
    checks = []

    def add_e(path: str):
        if path not in evidence:
            evidence.append(path)

    def add_c(path: str):
        if path not in checks:
            checks.append(path)

    add_e("docs/spec/CLI.md")
    add_c("scripts/check_upgrade_plan_health.sh")

    if "deterministic" in cap_l or "effect log" in cap_l or "sealed" in cap_l:
        add_e("docs/spec/SEALS_DISPATCH_REPLAY.md")
        add_e("docs/spec/DETERMINISM.md")
        add_c("scripts/check_no_user_panics.sh")
    if "coreform" in cap_l or "canonical" in cap_l:
        add_e("docs/spec/COREFORM_CANON_HASH.md")
    if "package" in cap_l or "gcpm" in cap_l or "workspace" in cap_l or "lock" in cap_l:
        add_e("docs/spec/GCPM_BUNDLE_v0.1.md")
        add_e("docs/spec/GCPM_JSON_SCHEMAS_v0.1.md")
        add_c("scripts/check_foundation_stdlib_conformance.sh")
    if "selfhost" in cap_l or "bootstrap" in cap_l or "artifact" in cap_l:
        add_e("docs/spec/SELF_HOST_BOUNDARY.md")
        add_c("scripts/check_selfhost_boundary.sh")
        add_c("scripts/check_selfhost_artifact_fresh.sh")
    if "concurrency" in cap_l or "multithreaded" in cap_l:
        add_e("docs/spec/CONCURRENCY_v0.1.md")
        add_c("scripts/check_task_concurrency_stress.sh")
    if "gpu" in cap_l or "gfx" in cap_l or "graphics" in cap_l:
        add_e("docs/spec/GPU_GFX_BUNDLE_v0.1.md")
        add_e("docs/spec/GPU_COMPUTE_RUNTIME_PROFILE_v0.1.md")
        add_c("scripts/check_gpu_compute_runtime_profile.sh")
        add_c("scripts/check_gpu_stack_decoupling.sh")
        add_c("scripts/check_gfx_runtime_profile.sh")
    if "webxr" in cap_l or "xr " in cap_l or "xr/" in cap_l:
        add_e("docs/spec/XR_HOST_RUNTIME_v0.1.md")
        add_c("scripts/check_webxr_browser_conformance_lane.sh")
    if "browser" in cap_l:
        add_e("docs/spec/BROWSER_HOST_RUNTIME_v0.1.md")
    if "plugin" in cap_l or "ffi" in cap_l:
        add_e("docs/spec/HOST_ABI.md")
        add_e("docs/spec/PLUGIN_ABI_SCHEMAS_v0.1.md")
        add_c("scripts/check_host_abi_conformance.sh")
    if "schema" in cap_l or "json cli" in cap_l or "machine-consumable" in cap_l:
        add_e("docs/spec/CLI_JSON_SCHEMAS_v0.1.md")
        add_c("scripts/check_cli_diagnostics_contract.sh")
    if "agent authoring contract" in cap_l or "machine-consumable agent authoring contract" in cap_l:
        add_e("docs/spec/WRITE_GENESISCODE_SKILL_PACK_v0.1.md")
        add_e("docs/spec/WRITE_GENESISCODE_SKILL_PACK_v0.1.json")
        add_c("scripts/check_write_genesiscode_skill_pack.sh")
        add_e("docs/spec/WRITE_GENESISCODE_SKILL_DISTRIBUTION_v1.md")
        add_c("scripts/check_write_genesiscode_skill_distribution.sh")
        add_c("scripts/check_write_genesiscode_skill_conformance.sh")
    if "agent" in cap_l or "workflow" in cap_l or "gauntlet" in cap_l:
        add_e("docs/spec/AGENT_AUTHORING_BUNDLE_v0.1.md")
        add_e("docs/spec/AGENT_CAPABILITY_GAUNTLET_v0.1.md")
        add_c("scripts/check_agent_reference_workflows.sh")
    if "requirements traceability" in cap_l or "coverage" in cap_l or "qualified" in cap_l or "assurance" in cap_l or "do-178c" in cap_l or "nasa" in cap_l or "iec" in cap_l:
        add_e("docs/spec/ASSURANCE_ARTIFACTS_v0.1.md")
        add_e("docs/spec/ASSURANCE_PROFILE_PACKS_v0.1.md")
        add_e("docs/spec/ASSURANCE_STANDARDS_CROSSWALK_v0.1.md")
        add_e("docs/spec/ASSURANCE_STANDARDS_CROSSWALK_v0.1.json")
        add_c("scripts/check_assurance_profile_packs.sh")
        add_c("scripts/check_assurance_standards_crosswalk.sh")
    if "runtime wall-time budgets" in cap_l or "perf" in cap_l or "hot-path" in cap_l or "profiling" in cap_l:
        add_e("docs/spec/TEST_EXECUTION_PROFILES_v0.1.md")
        add_c("scripts/check_perf_budgets.sh")

    # Claim-specific hard requirements to prevent evidence drift for high-impact rows.
    if cap == "GPU compute capability independent of graphics surface":
        add_e("docs/spec/GPU_COMPUTE_BUNDLE_v0.1.md")
        add_c("scripts/check_gpu_compute_runtime_profile.sh")
        add_c("scripts/check_gpu_stack_decoupling.sh")

    if cap == "Graphics/window/input/audio capability families":
        add_e("docs/spec/GFX_RUNTIME_BUNDLE_v0.1.md")
        add_c("scripts/check_gfx_runtime_profile.sh")

    if cap == "Deployment target pipeline in core toolchain":
        add_e("docs/spec/GCPM_JSON_SCHEMAS_v0.1.md")
        add_e("docs/spec/GCPM_WORKFLOW_REPORTS_v0.1.md")
        add_c("crates/gc_cli/tests/cli_pkg_workspace.rs")
        add_c("examples/agent_deploy_bundle_workflow/workflow.sh")

    return evidence, checks

entries = []
for cap in capabilities:
    evidence, checks = classify(cap)
    entries.append(
        {
            "capability": cap,
            "evidence_paths": evidence,
            "check_paths": checks,
        }
    )

obj = {
    "kind": "genesis/feature-matrix-evidence-v0.1",
    "version": "0.1",
    "source_feature_matrix": matrix_path.relative_to(root).as_posix(),
    "entry_count": len(entries),
    "entries": entries,
}

out_json.parent.mkdir(parents=True, exist_ok=True)
out_json.write_text(json.dumps(obj, indent=2, sort_keys=True) + "\n", encoding="utf-8")

md_lines = [
    "# Feature Matrix Evidence Ledger v0.1",
    "",
    "Machine-verifiable capability-to-evidence mapping generated from `feature_matrix.md`.",
    "",
    f"- Contract kind: `{obj['kind']}`",
    f"- Capability entries: `{len(entries)}`",
    f"- Source matrix: `{obj['source_feature_matrix']}`",
    "",
    "| Capability | Evidence Paths | Gate/Test Paths |",
    "| --- | --- | --- |",
]
for entry in entries:
    ev = "<br>".join(f"`{p}`" for p in entry["evidence_paths"])
    ck = "<br>".join(f"`{p}`" for p in entry["check_paths"])
    md_lines.append(f"| {entry['capability']} | {ev} | {ck} |")
out_md.parent.mkdir(parents=True, exist_ok=True)
out_md.write_text("\n".join(md_lines) + "\n", encoding="utf-8")

print(
    "update-feature-matrix-evidence: wrote "
    f"{out_json.relative_to(root).as_posix()} and {out_md.relative_to(root).as_posix()} "
    f"(entries={len(entries)})"
)
PY
