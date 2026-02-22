#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

GAUNTLET_REPORT="${GENESIS_WRITE_SKILL_GAUNTLET_REPORT:-.genesis/perf/agent_capability_gauntlet_report.json}"
GENERATIVE_REPORT="${GENESIS_WRITE_SKILL_GENERATIVE_REPORT:-.genesis/perf/agent_generative_workloads_report.json}"
REPORT_PATH="${GENESIS_WRITE_SKILL_CONFORMANCE_REPORT:-.genesis/perf/write_genesiscode_skill_conformance_report.json}"
HISTORY_PATH="${GENESIS_WRITE_SKILL_CONFORMANCE_HISTORY:-.genesis/perf/write_genesiscode_skill_conformance_history.jsonl}"
PROFILE="${GENESIS_WRITE_SKILL_CONFORMANCE_PROFILE:-${GENESIS_AGENT_GAUNTLET_PROFILE:-prepush-standard}}"
AUTO_RUN="${GENESIS_WRITE_SKILL_CONFORMANCE_AUTO_RUN:-1}"
MIN_SCORE="${GENESIS_WRITE_SKILL_CONFORMANCE_MIN_SCORE:-100}"
MIN_GENERATIVE_CASES="${GENESIS_WRITE_SKILL_CONFORMANCE_MIN_GENERATIVE_CASES:-8}"

if [[ "$AUTO_RUN" == "1" ]]; then
  if [[ ! -f "$GAUNTLET_REPORT" ]]; then
    GENESIS_AGENT_GAUNTLET_PROFILE="$PROFILE" \
      bash scripts/check_agent_reference_workflows.sh
  fi
  if [[ ! -f "$GENERATIVE_REPORT" ]]; then
    GENESIS_AGENT_GAUNTLET_PROFILE="$PROFILE" \
      bash scripts/check_agent_generative_workloads.sh
  fi
fi

[[ -f "$GAUNTLET_REPORT" ]] || {
  echo "write-genesiscode-skill-conformance: missing gauntlet report: $GAUNTLET_REPORT" >&2
  exit 1
}
[[ -f "$GENERATIVE_REPORT" ]] || {
  echo "write-genesiscode-skill-conformance: missing generative report: $GENERATIVE_REPORT" >&2
  exit 1
}

python3 - "$GAUNTLET_REPORT" "$GENERATIVE_REPORT" "$REPORT_PATH" "$HISTORY_PATH" "$MIN_SCORE" "$MIN_GENERATIVE_CASES" "$PROFILE" <<'PY'
import datetime as dt
import json
import pathlib
import sys

(
    gauntlet_path_s,
    generative_path_s,
    report_path_s,
    history_path_s,
    min_score_s,
    min_generative_cases_s,
    profile,
) = sys.argv[1:]

gauntlet_path = pathlib.Path(gauntlet_path_s)
generative_path = pathlib.Path(generative_path_s)
report_path = pathlib.Path(report_path_s)
history_path = pathlib.Path(history_path_s)
min_score = int(min_score_s)
min_generative_cases = int(min_generative_cases_s)

if min_score < 0 or min_score > 100:
    raise SystemExit("write-genesiscode-skill-conformance: min score must be in [0, 100]")
if min_generative_cases <= 0:
    raise SystemExit("write-genesiscode-skill-conformance: min generative cases must be > 0")

gauntlet = json.loads(gauntlet_path.read_text(encoding="utf-8"))
if gauntlet.get("kind") != "genesis/agent-capability-gauntlet-v0.1":
    raise SystemExit(
        f"write-genesiscode-skill-conformance: unexpected gauntlet kind: {gauntlet.get('kind')!r}"
    )

generative = json.loads(generative_path.read_text(encoding="utf-8"))
if generative.get("kind") != "genesis/agent-generative-workloads-v0.1":
    raise SystemExit(
        f"write-genesiscode-skill-conformance: unexpected generative kind: {generative.get('kind')!r}"
    )

workflows = {}
for row in gauntlet.get("workflows", []):
    if not isinstance(row, dict):
        continue
    name = row.get("name")
    if isinstance(name, str):
        workflows[name] = row

def check_workflow(
    *,
    rubric_id: str,
    workflow_names: list[str],
    required_domains: set[str],
    expected_weight: int,
):
    for name in workflow_names:
        row = workflows.get(name)
        if row is None:
            continue
        domains = set(row.get("domains") or [])
        missing_domains = sorted(required_domains - domains)
        pass_ok = (
            bool(row.get("ok", False))
            and bool(row.get("exit_ok", False))
            and bool(row.get("replay_signal", False))
            and bool(row.get("duration_ok", False))
            and not missing_domains
        )
        detail = {
            "rubric_id": rubric_id,
            "workflow": name,
            "ok": pass_ok,
            "weight": expected_weight,
            "score": expected_weight if pass_ok else 0,
            "required_domains": sorted(required_domains),
            "observed_domains": sorted(domains),
            "missing_domains": missing_domains,
            "replay_signal": bool(row.get("replay_signal", False)),
            "duration_ms": row.get("duration_ms"),
            "max_ms": row.get("max_ms"),
            "runtime_profile": gauntlet.get("runtime_profile"),
        }
        return detail
    return {
        "rubric_id": rubric_id,
        "workflow": None,
        "ok": False,
        "weight": expected_weight,
        "score": 0,
        "required_domains": sorted(required_domains),
        "observed_domains": [],
        "missing_domains": sorted(required_domains),
        "replay_signal": False,
        "duration_ms": None,
        "max_ms": None,
        "runtime_profile": gauntlet.get("runtime_profile"),
        "error": f"none of expected workflows present: {', '.join(workflow_names)}",
    }

rubric = [
    check_workflow(
        rubric_id="service",
        workflow_names=["agent_service_workflow"],
        required_domains={"service", "package_publish_sync"},
        expected_weight=20,
    ),
    check_workflow(
        rubric_id="game_loop",
        workflow_names=["agent_long_running_gfx_loop_workflow", "agent_interactive_gfx_compute_workflow"],
        required_domains={"graphics"},
        expected_weight=20,
    ),
    check_workflow(
        rubric_id="gpu_compute",
        workflow_names=["agent_gpu_compute_workflow", "agent_compute_workflow"],
        required_domains={"gpu_compute"},
        expected_weight=20,
    ),
    check_workflow(
        rubric_id="package_workflow",
        workflow_names=["agent_multi_package_publish_workflow"],
        required_domains={"package_publish_sync"},
        expected_weight=20,
    ),
]

generative_case_count = int(generative.get("case_count", 0))
generative_parity_mismatches = generative.get("parity_mismatches")
if not isinstance(generative_parity_mismatches, list):
    generative_parity_mismatches = []
generative_history_min_failures = generative.get("history_min_failures")
if not isinstance(generative_history_min_failures, list):
    generative_history_min_failures = []

generative_ok = (
    bool(generative.get("ok", False))
    and generative_case_count >= min_generative_cases
    and not generative_parity_mismatches
    and not generative_history_min_failures
)
rubric.append(
    {
        "rubric_id": "generative_mutation_suite",
        "workflow": "agent_generative_workloads",
        "ok": generative_ok,
        "weight": 20,
        "score": 20 if generative_ok else 0,
        "required_case_count": min_generative_cases,
        "observed_case_count": generative_case_count,
        "parity_mismatches": generative_parity_mismatches,
        "history_min_failures": generative_history_min_failures,
    }
)

total_score = sum(int(item["score"]) for item in rubric)
all_ok = all(bool(item["ok"]) for item in rubric)
threshold_ok = total_score >= min_score
ok = all_ok and threshold_ok

report = {
    "kind": "genesis/write-genesiscode-skill-conformance-v0.1",
    "ok": ok,
    "profile": profile,
    "runtime_profile": gauntlet.get("runtime_profile"),
    "gauntlet_report": str(gauntlet_path),
    "generative_report": str(generative_path),
    "min_score": min_score,
    "score": total_score,
    "threshold_ok": threshold_ok,
    "rubric": rubric,
    "timestamp_utc": dt.datetime.now(dt.timezone.utc).replace(microsecond=0).isoformat(),
}

report_path.parent.mkdir(parents=True, exist_ok=True)
report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")

history_path.parent.mkdir(parents=True, exist_ok=True)
history_entry = {
    "kind": report["kind"],
    "ok": ok,
    "profile": profile,
    "runtime_profile": gauntlet.get("runtime_profile"),
    "score": total_score,
    "min_score": min_score,
    "timestamp_utc": report["timestamp_utc"],
}
with history_path.open("a", encoding="utf-8") as fh:
    fh.write(json.dumps(history_entry, sort_keys=True) + "\n")

print(
    "write-genesiscode-skill-conformance: "
    f"report={report_path} ok={ok} score={total_score}/{min_score}"
)
if not ok:
    failures = [item["rubric_id"] for item in rubric if not bool(item["ok"])]
    raise SystemExit(
        "write-genesiscode-skill-conformance: failing rubric categories: "
        + ", ".join(failures)
        + f"; score={total_score} min_score={min_score}"
    )
PY
