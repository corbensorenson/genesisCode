#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if [[ "$#" -ne 3 ]]; then
  echo "usage: $0 <report-output> <gauntlet-report-input> <webxr-report-input>" >&2
  exit 2
fi

MANIFEST="docs/skill_pack/write_genesiscode_v1/manifest.json"
SPEC="docs/spec/GPU_COMPUTE_BUNDLE_v0.1.md"
REPORT_OUT="$1"
GAUNTLET_REPORT="$2"
WEBXR_REPORT="$3"
REQUIRE_WEBXR_RUNTIME_EVIDENCE="${GENESIS_GPU_XR_REQUIRE_WEBXR_RUNTIME_EVIDENCE:-1}"

[[ "$REQUIRE_WEBXR_RUNTIME_EVIDENCE" == "0" || "$REQUIRE_WEBXR_RUNTIME_EVIDENCE" == "1" ]] || {
  echo "gpu-xr-productization-kits: GENESIS_GPU_XR_REQUIRE_WEBXR_RUNTIME_EVIDENCE must be 0 or 1" >&2
  exit 2
}
required_files=(
  "$MANIFEST"
  "$SPEC"
  "docs/skill_pack/write_genesiscode_v1/recipes/gpu_compute_workflow.md"
  "docs/skill_pack/write_genesiscode_v1/recipes/xr_workflow.md"
  "examples/agent_compute_workflow/workflow.sh"
  "examples/agent_gpu_compute_workflow/workflow.sh"
  "examples/agent_xr_runtime_workflow/workflow.sh"
)
for path in "${required_files[@]}"; do
  [[ -f "$path" ]] || {
    echo "gpu-xr-productization-kits: missing required file: $path" >&2
    exit 1
  }
done

bash scripts/check_webxr_browser_conformance_lane.sh >/dev/null

python3 - "$ROOT_DIR" "$MANIFEST" "$SPEC" "$GAUNTLET_REPORT" "$WEBXR_REPORT" "$REPORT_OUT" "$REQUIRE_WEBXR_RUNTIME_EVIDENCE" <<'PY'
import json
import pathlib
import sys

root = pathlib.Path(sys.argv[1]).resolve()

def resolve_path(raw: str) -> pathlib.Path:
    path = pathlib.Path(raw)
    return path if path.is_absolute() else root / path

def stable_ref(path: pathlib.Path) -> str:
    try:
        return path.resolve().relative_to(root).as_posix()
    except ValueError:
        return f"external/{path.name}"

manifest_path = resolve_path(sys.argv[2])
spec_path = resolve_path(sys.argv[3])
gauntlet_path = resolve_path(sys.argv[4])
webxr_path = resolve_path(sys.argv[5])
report_path = resolve_path(sys.argv[6])
require_webxr_runtime_evidence = sys.argv[7] == "1"

manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
if manifest.get("kind") != "genesis/write-genesiscode-skill-distribution-v1":
    raise SystemExit("gpu-xr-productization-kits: manifest kind mismatch")

recipes = manifest.get("recipes")
if not isinstance(recipes, list):
    raise SystemExit("gpu-xr-productization-kits: manifest recipes must be an array")
recipes_by_id = {r.get("id"): r for r in recipes if isinstance(r, dict) and isinstance(r.get("id"), str)}

required_recipe_checks = {
    "gpu_data_simulation_workflow": {
        "domain": "gpu_non_graphics",
        "workflow": "examples/agent_compute_workflow/workflow.sh",
    },
    "xr_deploy_test_workflow": {
        "domain": "xr_productization",
        "workflow": "scripts/check_gpu_xr_productization_kits.sh",
    },
}

errors = []
recipe_details = {}
for rid, expected in required_recipe_checks.items():
    row = recipes_by_id.get(rid)
    if row is None:
        errors.append(f"missing-recipe:{rid}")
        continue
    domain = row.get("domain")
    workflow = row.get("workflow")
    ok = domain == expected["domain"] and workflow == expected["workflow"]
    recipe_details[rid] = {
        "ok": ok,
        "domain": domain,
        "workflow": workflow,
        "expected_domain": expected["domain"],
        "expected_workflow": expected["workflow"],
    }
    if not ok:
        errors.append(f"recipe-mismatch:{rid}")

spec_text = spec_path.read_text(encoding="utf-8")
for token in [
    "gpu_data_simulation_workflow",
    "xr_deploy_test_workflow",
    "examples/agent_compute_workflow/workflow.sh",
    "examples/agent_gpu_compute_workflow/workflow.sh",
    "examples/agent_xr_runtime_workflow/workflow.sh",
    "scripts/check_webxr_browser_conformance.sh",
]:
    if token not in spec_text:
        errors.append(f"spec-missing:{token}")

workflow_rows = {}
if not gauntlet_path.is_file():
    errors.append("gauntlet-report-missing")
else:
    gauntlet = json.loads(gauntlet_path.read_text(encoding="utf-8"))
    if gauntlet.get("kind") != "genesis/agent-capability-gauntlet-v0.1":
        errors.append("gauntlet-kind-mismatch")
    else:
        rows = gauntlet.get("workflows")
        if not isinstance(rows, list):
            errors.append("gauntlet-workflows-missing")
        else:
            rows_map = {
                r.get("name"): r
                for r in rows
                if isinstance(r, dict) and isinstance(r.get("name"), str)
            }
            for wf in [
                "agent_compute_workflow",
                "agent_gpu_compute_workflow",
                "agent_xr_runtime_workflow",
            ]:
                row = rows_map.get(wf)
                if row is None:
                    errors.append(f"gauntlet-workflow-missing:{wf}")
                    continue
                wf_ok = bool(row.get("exit_ok", False)) and bool(row.get("replay_signal", False))
                workflow_rows[wf] = {
                    "ok": wf_ok,
                    "exit_ok": bool(row.get("exit_ok", False)),
                    "replay_signal": bool(row.get("replay_signal", False)),
                    "gpu_backend": row.get("gpu_backend"),
                }
                if not wf_ok:
                    errors.append(f"gauntlet-workflow-not-deterministic:{wf}")

webxr = None
if webxr_path.is_file():
    webxr = json.loads(webxr_path.read_text(encoding="utf-8"))
    if webxr.get("kind") != "genesis/webxr-browser-conformance-v0.1":
        errors.append("webxr-kind-mismatch")
    else:
        if not bool(webxr.get("ok", False)):
            errors.append("webxr-not-ok")
        if not bool(webxr.get("deterministic_replay", False)):
            errors.append("webxr-nondeterministic")
else:
    if require_webxr_runtime_evidence:
        errors.append("webxr-report-missing")

report = {
    "kind": "genesis/gpu-xr-productization-kits-v0.1",
    "manifest_path": stable_ref(manifest_path),
    "spec_path": stable_ref(spec_path),
    "gauntlet_report": stable_ref(gauntlet_path),
    "webxr_report": stable_ref(webxr_path),
    "required_webxr_runtime_evidence": require_webxr_runtime_evidence,
    "recipe_checks": recipe_details,
    "workflow_checks": workflow_rows,
    "webxr_runtime_evidence_present": bool(webxr_path.is_file()),
    "webxr_deterministic_replay": bool(webxr and webxr.get("deterministic_replay", False)),
    "ok": not errors,
    "errors": errors,
}
report_path.parent.mkdir(parents=True, exist_ok=True)
report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")

if errors:
    raise SystemExit("gpu-xr-productization-kits: " + " | ".join(errors))

print(
    "gpu-xr-productization-kits: ok "
    f"(recipes={len(recipe_details)} workflows={len(workflow_rows)} webxr_evidence={report['webxr_runtime_evidence_present']})"
)
PY
