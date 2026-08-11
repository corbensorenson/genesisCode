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
EXPLAIN_ID="$(python3 - "$TMP_DIR/slice.json" "$TMP_DIR/rendered.json" <<'PY'
import json
from pathlib import Path
import sys

execution_slice = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
manifest = json.loads(Path(sys.argv[2]).read_text(encoding="utf-8"))
focus_ids = [task["id"] for task in execution_slice["focus_tasks"]]
if focus_ids:
    print(focus_ids[0])
else:
    open_ids = [task["id"] for task in manifest["tasks"] if task["state"] == "open"]
    print(open_ids[0] if open_ids else manifest["tasks"][-1]["id"])
PY
)"
python3 scripts/lib/roadmap_execution_manifest.py --explain "$EXPLAIN_ID" >"$TMP_DIR/explain.json"

python3 - "$TMP_DIR/slice.json" "$TMP_DIR/ready.json" "$TMP_DIR/explain.json" "$TMP_DIR/rendered.json" "$EXPLAIN_ID" <<'PY'
import json
from pathlib import Path
import sys

execution_slice = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
readiness = json.loads(Path(sys.argv[2]).read_text(encoding="utf-8"))
explanation = json.loads(Path(sys.argv[3]).read_text(encoding="utf-8"))
manifest = json.loads(Path(sys.argv[4]).read_text(encoding="utf-8"))
explain_id = sys.argv[5]
policy = json.loads(
    Path("policies/roadmap_execution_v0.1.json").read_text(encoding="utf-8")
)
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
if len(execution_slice["allowed_parallel_lanes"]) != 1:
    raise SystemExit("roadmap-execution-manifest: allowed parallel lane drift")
parallel_lanes = {lane["id"]: lane for lane in execution_slice["allowed_parallel_lanes"]}
if set(parallel_lanes) != {"read-only-selfhost-assurance"}:
    raise SystemExit("roadmap-execution-manifest: allowed parallel lane contract drift")
parallel_lane = parallel_lanes["read-only-selfhost-assurance"]
parallel_contract = " ".join(parallel_lane["conditions"])
for forbidden in (
    "directly satisfies the selected GenesisCode task's declared evidence contract",
    "runs on separately provisioned compute and storage",
    "cannot consume the active task's local CPU, memory, disk, network, lock, or agent-attention budget",
    "cannot modify repository files",
    "performs no target-model inference, benchmark custody or commissioning, result publication, Foundry implementation",
    "cannot authorize completion",
):
    if forbidden not in parallel_contract:
        raise SystemExit(
            f"roadmap-execution-manifest: read-only parallel lane lost {forbidden!r}"
        )

tasks = {task["id"]: task for task in manifest["tasks"]}

def closure(root):
    seen = set()
    pending = [root]
    while pending:
        task_id = pending.pop()
        if task_id in seen:
            continue
        seen.add(task_id)
        pending.extend(tasks[task_id]["prerequisites"])
    return seen

unfinished_preview = {"R1.4.o", "R1.4.p", "R1.4.q", "R1.4.r"}
core = closure("R9.4.f")
if unfinished_preview & core or any(
    task_id.startswith(("R8.5.", "F")) for task_id in core
):
    raise SystemExit("roadmap-execution-manifest: GenesisCode Core absorbed a post-Core lane")
ordered_anchors = policy["execution_frontier"]["ordered_task_ids"]
if not set(ordered_anchors).issubset(core):
    raise SystemExit("roadmap-execution-manifest: pre-Core frontier contains a post-Core anchor")
tasks_by_workstream = {}
for task in manifest["tasks"]:
    tasks_by_workstream.setdefault(task["workstream"], []).append(task)
covered_open_core = set()
for anchor_id in ordered_anchors:
    anchor = tasks[anchor_id]
    members = tasks_by_workstream[anchor["workstream"]]
    if policy["workstreams"][anchor["workstream"]]["sequential"]:
        anchor_index = next(
            index for index, member in enumerate(members) if member["id"] == anchor_id
        )
        candidates = members[anchor_index:]
    else:
        candidates = [anchor]
    covered_open_core.update(
        task["id"]
        for task in candidates
        if task["state"] == "open" and task["id"] in core
    )
open_core = {task_id for task_id in core if tasks[task_id]["state"] == "open"}
missing_frontier = sorted(open_core - covered_open_core)
if missing_frontier:
    raise SystemExit(
        "roadmap-execution-manifest: Core frontier is not continuation-complete: "
        + ", ".join(missing_frontier)
    )
foundry_foundation = closure("F2.r")
if not {"R9.4.f", "F2.q"}.issubset(foundry_foundation) or unfinished_preview & foundry_foundation or any(
    task_id.startswith("R8.5.") for task_id in foundry_foundation
):
    raise SystemExit("roadmap-execution-manifest: Foundry Foundation is not Core-only")
challenge = closure("R8.5.v")
if not {"R9.4.f", "F2.r", "R8.5.u"}.issubset(challenge):
    raise SystemExit("roadmap-execution-manifest: GenesisChallenge lost its Foundation handoff")
if unfinished_preview & challenge or any(
    (task_id.startswith("F") and task_id not in foundry_foundation)
    or (task_id.startswith("R8.5.") and task_id not in {"R8.5.u", "R8.5.v"})
    for task_id in challenge
):
    raise SystemExit("roadmap-execution-manifest: GenesisChallenge absorbed another post-Core lane")
if "R8.5.s" not in closure("F2.y"):
    raise SystemExit("roadmap-execution-manifest: Foundry integration bypassed Benchmark Trust")
model_readiness = closure("R8.5.t")
if not {"R8.5.s", "F2.r"}.issubset(model_readiness) or {
    "R8.5.u", "R8.5.v"
} & model_readiness:
    raise SystemExit("roadmap-execution-manifest: Genesis Model readiness lane drift")
focus_ids = [task["id"] for task in execution_slice["focus_tasks"]]
explained_task = tasks[explain_id]
if explanation["id"] != explain_id or any(
    explanation[field] != explained_task[field]
    for field in ("state", "start_ready")
):
    raise SystemExit("roadmap-execution-manifest: task explanation drift")
if focus_ids and explain_id != focus_ids[0]:
    raise SystemExit("roadmap-execution-manifest: explanation does not cover selected focus")
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
    or readiness["frontierFocusTaskIds"] != focus_ids
    or readiness["selectedReadyTaskIds"] != focus_ids
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
expected_ready_ids = [
    task["id"]
    for task in manifest["tasks"]
    if task["state"] == "open" and task["start_ready"]
]
if ready_ids != expected_ready_ids:
    raise SystemExit("roadmap-execution-manifest: global readiness set drift")
for task in readiness["startReadyTasks"]:
    selected = task["id"] in focus_ids
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

cp policies/roadmap_execution_v0.1.json "$TMP_DIR/post-core-edge-bypassed-policy.json"
python3 - "$TMP_DIR/post-core-edge-bypassed-policy.json" <<'PY'
import json
from pathlib import Path
import sys

path = Path(sys.argv[1])
policy = json.loads(path.read_text(encoding="utf-8"))
policy["task_prerequisites"]["F2.y"] = []
path.write_text(json.dumps(policy, indent=2) + "\n", encoding="utf-8")
PY
if python3 scripts/lib/roadmap_execution_manifest.py \
  --render \
  --policy "$TMP_DIR/post-core-edge-bypassed-policy.json" \
  --output "$TMP_DIR/rejected.json" >/dev/null 2>&1; then
  echo "roadmap-execution-manifest: Foundry integration bypassed Benchmark Trust" >&2
  exit 1
fi

cp policies/roadmap_execution_v0.1.json "$TMP_DIR/core-frontier-truncated-policy.json"
python3 - "$TMP_DIR/core-frontier-truncated-policy.json" <<'PY'
import json
from pathlib import Path
import sys

path = Path(sys.argv[1])
policy = json.loads(path.read_text(encoding="utf-8"))
policy["execution_frontier"]["ordered_task_ids"].remove("R9.4.a")
del policy["execution_frontier"]["task_context"]["R9.4.a"]
path.write_text(json.dumps(policy, indent=2) + "\n", encoding="utf-8")
PY
if python3 scripts/lib/roadmap_execution_manifest.py \
  --render \
  --policy "$TMP_DIR/core-frontier-truncated-policy.json" \
  --output "$TMP_DIR/rejected.json" >/dev/null 2>&1; then
  echo "roadmap-execution-manifest: truncated Core frontier was accepted" >&2
  exit 1
fi

cp policies/roadmap_execution_v0.1.json "$TMP_DIR/parallel-lane-broadened-policy.json"
python3 - "$TMP_DIR/parallel-lane-broadened-policy.json" <<'PY'
import json
from pathlib import Path
import sys

path = Path(sys.argv[1])
policy = json.loads(path.read_text(encoding="utf-8"))
lane = next(
    lane for lane in policy["execution_frontier"]["allowed_parallel_lanes"]
    if lane["id"] == "read-only-selfhost-assurance"
)
lane["conditions"] = [
    condition.replace("runs on separately provisioned compute and storage", "shares local compute and storage")
    for condition in lane["conditions"]
]
path.write_text(json.dumps(policy, indent=2) + "\n", encoding="utf-8")
PY
if python3 scripts/lib/roadmap_execution_manifest.py \
  --render \
  --policy "$TMP_DIR/parallel-lane-broadened-policy.json" \
  --output "$TMP_DIR/rejected.json" >/dev/null 2>&1; then
  echo "roadmap-execution-manifest: resource-sharing parallel lane was accepted" >&2
  exit 1
fi

before="$(cksum docs/program/ROADMAP_EXECUTION_MANIFEST_v0.1.json)"
python3 scripts/lib/roadmap_execution_manifest.py --check
after="$(cksum docs/program/ROADMAP_EXECUTION_MANIFEST_v0.1.json)"
[[ "$before" == "$after" ]] || {
  echo "roadmap-execution-manifest: check mode mutated the retained manifest" >&2
  exit 1
}

echo "roadmap-execution-manifest-contract: ok (negative_controls=30 query_views=2 lane_isolation=17 post_core_edges=3 core_frontier=complete parallel_lanes=1 validation_economy=active scope_freeze=active check_mode=read_only)"
