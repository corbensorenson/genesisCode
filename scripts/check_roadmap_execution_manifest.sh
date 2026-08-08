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
python3 scripts/lib/roadmap_execution_manifest.py --explain R0.4.j >"$TMP_DIR/explain.json"

python3 - "$TMP_DIR/slice.json" "$TMP_DIR/explain.json" <<'PY'
import json
from pathlib import Path
import sys

execution_slice = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
explanation = json.loads(Path(sys.argv[2]).read_text(encoding="utf-8"))
if execution_slice["kind"] != "genesis/roadmap-execution-slice-v0.1":
    raise SystemExit("roadmap-execution-manifest: execution slice kind drift")
if execution_slice["authority"]["derived_view_only"] is not True:
    raise SystemExit("roadmap-execution-manifest: execution slice claims authority")
if len(execution_slice["focus_tasks"]) > execution_slice["wip_limit"]:
    raise SystemExit("roadmap-execution-manifest: execution slice exceeds WIP limit")
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
    explanation["id"] != "R0.4.j"
    or explanation["state"] != "open"
    or explanation["start_ready"] is not True
):
    raise SystemExit("roadmap-execution-manifest: task explanation drift")
if [task["id"] for task in execution_slice["focus_tasks"]] != ["R0.4.j"]:
    raise SystemExit("roadmap-execution-manifest: corrective focus drift")
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

echo "roadmap-execution-manifest-contract: ok (negative_controls=28 query_views=2 lane_isolation=13 parallel_lanes=2 scope_freeze=active check_mode=read_only)"
