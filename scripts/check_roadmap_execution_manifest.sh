#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

python3 scripts/lib/roadmap_execution_manifest.py --self-test >/dev/null
python3 scripts/lib/roadmap_execution_manifest.py \
  --render \
  --output "$TMP_DIR/rendered.json" >/dev/null
python3 scripts/lib/roadmap_execution_manifest.py --slice >"$TMP_DIR/slice.json"
python3 scripts/lib/roadmap_execution_manifest.py --ready >"$TMP_DIR/ready.json"
python3 scripts/lib/roadmap_execution_manifest.py --explain R2.2.f >"$TMP_DIR/explain.json"

python3 - "$TMP_DIR/slice.json" "$TMP_DIR/ready.json" "$TMP_DIR/explain.json" <<'PY'
import json
from pathlib import Path
import sys

execution_slice = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
readiness = json.loads(Path(sys.argv[2]).read_text(encoding="utf-8"))
explanation = json.loads(Path(sys.argv[3]).read_text(encoding="utf-8"))
if execution_slice["kind"] != "genesis/roadmap-execution-slice-v0.1":
    raise SystemExit("roadmap-execution-manifest: execution slice kind drift")
if execution_slice["authority"]["derived_view_only"] is not True:
    raise SystemExit("roadmap-execution-manifest: execution slice claims authority")
if len(execution_slice["focus_tasks"]) > execution_slice["wip_limit"]:
    raise SystemExit("roadmap-execution-manifest: execution slice exceeds WIP limit")
validation = execution_slice.get("validation_economy")
if validation != {
    "identical_success_limit_per_exact_identity": 1,
    "additional_identical_run_condition": "recorded-flake-or-nondeterminism-hypothesis",
    "release_calibration_task_id": "R9.1.c",
    "whole_profile_sampling": "one-outer-invocation-after-inner-harness-pass",
    "long_running_supervision": "autonomous-state-transitions-only",
    "subject_readiness_order": ["contract", "focused", "integration", "assurance", "release"],
    "required_campaign_fields": [
        "decision", "subject-readiness", "independent-variable", "observation-reuse",
        "resource-budget", "stopping-rule", "terminal-artifact",
    ],
}:
    raise SystemExit("roadmap-execution-manifest: validation economy drift")
required = {
    "id", "title", "phase", "workstream", "product_lanes", "milestones",
    "start_ready", "prerequisites", "unsatisfied_prerequisites", "risk_class",
    "resource_class", "owner_paths", "guard_checks", "parallel_safe_with",
    "negative_controls", "nonclaims", "expected_outputs", "acceptance", "rollback",
    "source",
}
for task in execution_slice["focus_tasks"]:
    if set(task) != required:
        raise SystemExit("roadmap-execution-manifest: execution slice task field drift")
    if not task["product_lanes"] or not task["milestones"] or not task["nonclaims"]:
        raise SystemExit("roadmap-execution-manifest: execution slice context is incomplete")
if [task["id"] for task in execution_slice["queued_tasks"]] != execution_slice["queued_task_ids"]:
    raise SystemExit("roadmap-execution-manifest: queued task context drift")
for task in execution_slice["queued_tasks"]:
    if not task["product_lanes"] or not task["milestones"] or not task["nonclaims"]:
        raise SystemExit("roadmap-execution-manifest: queued task context is incomplete")
scope_freeze = execution_slice.get("scope_freeze")
if not isinstance(scope_freeze, dict) or scope_freeze.get("until_task_id") != "R9.4.f":
    raise SystemExit("roadmap-execution-manifest: scope freeze drift")
if scope_freeze.get("frozen_program_concepts") != [
    "GenesisCode", "GenesisBench", "GenesisChallenge", "Genesis Foundry", "Genesis Model"
]:
    raise SystemExit("roadmap-execution-manifest: frozen program concept drift")
if len(execution_slice["allowed_parallel_lanes"]) != 2:
    raise SystemExit("roadmap-execution-manifest: allowed parallel lane drift")
parallel_lanes = {lane["id"]: lane for lane in execution_slice["allowed_parallel_lanes"]}
if set(parallel_lanes) != {"read-only-selfhost-assurance", "model-interface-portability-canary"}:
    raise SystemExit("roadmap-execution-manifest: allowed parallel lane contract drift")
parallel_lane = parallel_lanes["read-only-selfhost-assurance"]
parallel_contract = " ".join(parallel_lane["conditions"])
for forbidden in (
    "cannot modify repository files",
    "performs no target-model inference, benchmark custody or commissioning, result publication, Foundry implementation",
    "cannot authorize completion",
):
    if forbidden not in parallel_contract:
        raise SystemExit(
            f"roadmap-execution-manifest: read-only parallel lane lost {forbidden!r}"
        )
canary_contract = " ".join(parallel_lanes["model-interface-portability-canary"]["conditions"])
for required in (
    "no GenesisBench task, private payload, scorer",
    "cannot modify repository files",
    "creates no benchmark attempt, score, cohort, rank, result",
):
    if required not in canary_contract:
        raise SystemExit(
            f"roadmap-execution-manifest: portability canary lost {required!r}"
        )
if (
    explanation["id"] != "R2.2.f"
    or explanation["state"] != "open"
    or explanation["start_ready"] is not True
):
    raise SystemExit("roadmap-execution-manifest: task explanation drift")
if [task["id"] for task in execution_slice["focus_tasks"]] != ["R2.2.f"]:
    raise SystemExit("roadmap-execution-manifest: corrective focus drift")
focus = execution_slice["focus_tasks"][0]
if focus["resource_class"] != "build":
    raise SystemExit("roadmap-execution-manifest: lifecycle focus is not build-class work")
required_lifecycle_guards = {
    "scripts/check_host_bridge_fault_injection.sh",
    "scripts/check_no_user_panics.sh",
    "scripts/check_host_abi_conformance.sh",
}
if not required_lifecycle_guards.issubset(focus["guard_checks"]):
    raise SystemExit("roadmap-execution-manifest: lifecycle focus lost task-specific guards")
if {
    "scripts/check_runtime_workload_budgets.sh",
    "scripts/check_perf_budgets.sh",
}.intersection(focus["guard_checks"]):
    raise SystemExit("roadmap-execution-manifest: lifecycle focus inherited performance guards")
if set(readiness) != {
    "kind", "version", "authority", "inputIdentities", "wipLimit",
    "openTaskCount", "startReadyTaskCount", "frontierFocusTaskIds",
    "selectedReadyTaskIds", "startReadyTasks", "nonclaims",
}:
    raise SystemExit("roadmap-execution-manifest: readiness report field drift")
if (
    readiness["kind"] != "genesis/roadmap-start-readiness-v0.1"
    or readiness["authority"] != {
        "derivedViewOnly": True,
        "policy": "policies/roadmap_execution_v0.1.json",
        "roadmap": "ROADMAP.md",
        "selector": "--slice",
    }
    or readiness["wipLimit"] != execution_slice["wip_limit"]
    or readiness["frontierFocusTaskIds"] != ["R2.2.f"]
    or readiness["selectedReadyTaskIds"] != ["R2.2.f"]
    or readiness["startReadyTaskCount"] != len(readiness["startReadyTasks"])
):
    raise SystemExit("roadmap-execution-manifest: readiness report derivation drift")
ready_fields = {
    "id", "title", "phase", "workstream", "riskClass", "resourceClass",
    "prerequisites", "selectedByFrontier", "selectionDisposition",
    "deprioritizedReason", "source",
}
if any(set(task) != ready_fields for task in readiness["startReadyTasks"]):
    raise SystemExit("roadmap-execution-manifest: readiness task field drift")
ready_ids = [task["id"] for task in readiness["startReadyTasks"]]
if ready_ids != ["R1.5.c", "R2.1.h", "R2.2.f", "R4.1.a", "R7.1.a"]:
    raise SystemExit("roadmap-execution-manifest: global readiness set drift")
for task in readiness["startReadyTasks"]:
    selected = task["id"] == "R2.2.f"
    if (
        task["selectedByFrontier"] is not selected
        or task["selectionDisposition"]
        != ("selected" if selected else "ready-but-deprioritized")
        or task["deprioritizedReason"]
        != (None if selected else "wip-limit-and-frontier-priority")
    ):
        raise SystemExit("roadmap-execution-manifest: readiness selection drift")
PY

if python3 scripts/lib/roadmap_execution_manifest.py --explain R99.99.z \
    >"$TMP_DIR/unknown.out" 2>"$TMP_DIR/unknown.err"; then
  echo "roadmap-execution-manifest: unknown task explanation was accepted" >&2
  exit 1
fi

if ! cmp -s docs/program/ROADMAP_EXECUTION_MANIFEST_v0.1.json "$TMP_DIR/rendered.json"; then
  echo "roadmap-execution-manifest: generated manifest drift" >&2
  echo "roadmap-execution-manifest: run bash scripts/update_roadmap_execution_manifest.sh" >&2
  exit 1
fi

cp policies/roadmap_execution_v0.1.json "$TMP_DIR/duplicate-key-policy.json"
python3 - "$TMP_DIR/duplicate-key-policy.json" <<'PY'
from pathlib import Path
import sys

path = Path(sys.argv[1])
text = path.read_text(encoding="utf-8")
needle = '  "version": "0.1",\n'
if text.count(needle) != 1:
    raise SystemExit("roadmap-execution-manifest: duplicate-key fixture anchor drift")
path.write_text(text.replace(needle, needle + needle, 1), encoding="utf-8")
PY
if python3 scripts/lib/roadmap_execution_manifest.py \
  --render \
  --policy "$TMP_DIR/duplicate-key-policy.json" \
  --output "$TMP_DIR/rejected.json" >/dev/null 2>&1; then
  echo "roadmap-execution-manifest: duplicate-key policy fixture was accepted" >&2
  exit 1
fi

cp policies/roadmap_execution_v0.1.json "$TMP_DIR/validation-economy-broadened-policy.json"
python3 - "$TMP_DIR/validation-economy-broadened-policy.json" <<'PY'
import json
from pathlib import Path
import sys

path = Path(sys.argv[1])
policy = json.loads(path.read_text(encoding="utf-8"))
policy["execution_frontier"]["validation_economy"]["identical_success_limit_per_exact_identity"] = 2
path.write_text(json.dumps(policy, indent=2) + "\n", encoding="utf-8")
PY
if python3 scripts/lib/roadmap_execution_manifest.py \
  --render \
  --policy "$TMP_DIR/validation-economy-broadened-policy.json" \
  --output "$TMP_DIR/rejected.json" >/dev/null 2>&1; then
  echo "roadmap-execution-manifest: broadened validation economy was accepted" >&2
  exit 1
fi

cp policies/roadmap_execution_v0.1.json "$TMP_DIR/unknown-task-profile-policy.json"
python3 - "$TMP_DIR/unknown-task-profile-policy.json" <<'PY'
import json
from pathlib import Path
import sys

path = Path(sys.argv[1])
policy = json.loads(path.read_text(encoding="utf-8"))
policy["task_execution_profiles"]["R2.2.f"] = "unknown-profile"
path.write_text(json.dumps(policy, indent=2) + "\n", encoding="utf-8")
PY
if python3 scripts/lib/roadmap_execution_manifest.py \
  --render \
  --policy "$TMP_DIR/unknown-task-profile-policy.json" \
  --output "$TMP_DIR/rejected.json" >/dev/null 2>&1; then
  echo "roadmap-execution-manifest: unknown task execution profile was accepted" >&2
  exit 1
fi

cp policies/roadmap_execution_v0.1.json "$TMP_DIR/scope-broadened-policy.json"
python3 - "$TMP_DIR/scope-broadened-policy.json" <<'PY'
import json
from pathlib import Path
import sys

path = Path(sys.argv[1])
policy = json.loads(path.read_text(encoding="utf-8"))
policy["execution_frontier"]["scope_freeze"]["frozen_program_concepts"].append("New Program")
path.write_text(json.dumps(policy, indent=2) + "\n", encoding="utf-8")
PY
if python3 scripts/lib/roadmap_execution_manifest.py \
  --render \
  --policy "$TMP_DIR/scope-broadened-policy.json" \
  --output "$TMP_DIR/rejected.json" >/dev/null 2>&1; then
  echo "roadmap-execution-manifest: broadened conceptual scope was accepted" >&2
  exit 1
fi

cp policies/roadmap_execution_v0.1.json "$TMP_DIR/canary-broadened-policy.json"
python3 - "$TMP_DIR/canary-broadened-policy.json" <<'PY'
import json
from pathlib import Path
import sys

path = Path(sys.argv[1])
policy = json.loads(path.read_text(encoding="utf-8"))
lane = next(
    lane for lane in policy["execution_frontier"]["allowed_parallel_lanes"]
    if lane["id"] == "model-interface-portability-canary"
)
lane["conditions"] = [
    condition.replace("no GenesisBench task, private payload, scorer", "fixed public task and scorer")
    for condition in lane["conditions"]
]
path.write_text(json.dumps(policy, indent=2) + "\n", encoding="utf-8")
PY
if python3 scripts/lib/roadmap_execution_manifest.py \
  --render \
  --policy "$TMP_DIR/canary-broadened-policy.json" \
  --output "$TMP_DIR/rejected.json" >/dev/null 2>&1; then
  echo "roadmap-execution-manifest: scoring portability canary was accepted" >&2
  exit 1
fi

before="$(cksum docs/program/ROADMAP_EXECUTION_MANIFEST_v0.1.json)"
python3 scripts/lib/roadmap_execution_manifest.py --check
after="$(cksum docs/program/ROADMAP_EXECUTION_MANIFEST_v0.1.json)"
[[ "$before" == "$after" ]] || {
  echo "roadmap-execution-manifest: check mode mutated the retained manifest" >&2
  exit 1
}

echo "roadmap-execution-manifest-contract: ok (negative_controls=30 query_views=2 lane_isolation=13 parallel_lanes=2 validation_economy=active scope_freeze=active check_mode=read_only)"
