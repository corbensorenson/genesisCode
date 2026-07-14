#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if [[ "$#" -ne 5 ]]; then
  echo "usage: $0 <report-output> <history-output> <history-input> <primary-gauntlet-input> <secondary-gauntlet-input>" >&2
  exit 2
fi

REPORT_PATH="$1"
HISTORY_PATH="$2"
HISTORY_INPUT_PATH="$3"
PRIMARY_REPORT="$4"
SECONDARY_REPORT="$5"
REQUIRE_SECONDARY="${GENESIS_AGENT_GENERATIVE_REQUIRE_SECONDARY:-1}"
BASELINE_HISTORY_PATH="${GENESIS_AGENT_GENERATIVE_BASELINE_HISTORY:-policies/perf/agent_generative_workloads_seed_history.jsonl}"
CASE_COUNT="${GENESIS_AGENT_GENERATIVE_CASE_COUNT:-100}"
MIN_WORKFLOWS="${GENESIS_AGENT_GENERATIVE_MIN_WORKFLOWS:-3}"
MAX_WORKFLOWS="${GENESIS_AGENT_GENERATIVE_MAX_WORKFLOWS:-6}"
MIN_DOMAIN_COUNT="${GENESIS_AGENT_GENERATIVE_MIN_DOMAIN_COUNT:-2}"
MAX_CASE_DURATION_MS="${GENESIS_AGENT_GENERATIVE_MAX_CASE_DURATION_MS:-600000}"
P95_MIN_SAMPLES="${GENESIS_AGENT_GENERATIVE_P95_MIN_SAMPLES:-8}"
REGRESSION_PERCENT="${GENESIS_AGENT_GENERATIVE_REGRESSION_PERCENT:-60}"
REGRESSION_SLACK_MS="${GENESIS_AGENT_GENERATIVE_REGRESSION_SLACK_MS:-3000}"
REQUIRE_MIN_HISTORY="${GENESIS_AGENT_GENERATIVE_REQUIRE_MIN_HISTORY:-1}"
SEED="${GENESIS_AGENT_GENERATIVE_SEED:-genesis-agent-generative-v1}"

if [[ "$REQUIRE_SECONDARY" != "0" && "$REQUIRE_SECONDARY" != "1" ]]; then
  echo "agent-generative-workloads: GENESIS_AGENT_GENERATIVE_REQUIRE_SECONDARY must be 0 or 1" >&2
  exit 2
fi
if [[ ! "$REGRESSION_SLACK_MS" =~ ^[0-9]+$ ]]; then
  echo "agent-generative-workloads: GENESIS_AGENT_GENERATIVE_REGRESSION_SLACK_MS must be a non-negative integer" >&2
  exit 2
fi
if [[ "$REQUIRE_SECONDARY" == "1" && -z "$SECONDARY_REPORT" ]]; then
  echo "agent-generative-workloads: secondary report is required but not configured" >&2
  exit 2
fi
python3 - "$PRIMARY_REPORT" "$SECONDARY_REPORT" "$REPORT_PATH" "$HISTORY_PATH" "$HISTORY_INPUT_PATH" "$BASELINE_HISTORY_PATH" "$CASE_COUNT" "$MIN_WORKFLOWS" "$MAX_WORKFLOWS" "$MIN_DOMAIN_COUNT" "$MAX_CASE_DURATION_MS" "$P95_MIN_SAMPLES" "$REGRESSION_PERCENT" "$REGRESSION_SLACK_MS" "$REQUIRE_MIN_HISTORY" "$REQUIRE_SECONDARY" "$SEED" <<'PY'
import datetime as dt
import hashlib
import json
import math
import pathlib
import random
import statistics
import sys

(
    primary_report_path,
    secondary_report_path,
    report_path,
    history_path,
    history_input_path,
    baseline_history_path,
    case_count_s,
    min_workflows_s,
    max_workflows_s,
    min_domain_count_s,
    max_case_duration_ms_s,
    p95_min_samples_s,
    regression_percent_s,
    regression_slack_ms_s,
    require_min_history_raw,
    require_secondary_raw,
    seed,
) = sys.argv[1:]

case_count = int(case_count_s)
min_workflows = int(min_workflows_s)
max_workflows = int(max_workflows_s)
min_domain_count = int(min_domain_count_s)
max_case_duration_ms = int(max_case_duration_ms_s)
p95_min_samples = int(p95_min_samples_s)
regression_percent = float(regression_percent_s)
regression_slack_ms = int(regression_slack_ms_s)
require_min_history = require_min_history_raw.strip().lower() not in {"0", "false", "no", "off"}
require_secondary = require_secondary_raw.strip().lower() not in {"0", "false", "no", "off"}

if case_count <= 0:
    raise SystemExit("agent-generative-workloads: case count must be positive")
if min_workflows <= 0 or max_workflows <= 0:
    raise SystemExit("agent-generative-workloads: min/max workflows must be positive")
if min_workflows > max_workflows:
    raise SystemExit("agent-generative-workloads: min workflows cannot exceed max workflows")
if min_domain_count <= 0:
    raise SystemExit("agent-generative-workloads: min domain count must be positive")
if max_case_duration_ms <= 0:
    raise SystemExit("agent-generative-workloads: max case duration must be positive")
if p95_min_samples <= 0:
    raise SystemExit("agent-generative-workloads: p95 min samples must be positive")
if regression_percent < 0:
    raise SystemExit("agent-generative-workloads: regression percent must be non-negative")
if regression_slack_ms < 0:
    raise SystemExit("agent-generative-workloads: regression slack must be non-negative")

primary_path = pathlib.Path(primary_report_path)
secondary_path = pathlib.Path(secondary_report_path) if secondary_report_path.strip() else None
report_file = pathlib.Path(report_path)
history_file = pathlib.Path(history_path)
history_input_file = pathlib.Path(history_input_path)
baseline_history_file = pathlib.Path(baseline_history_path)
if require_secondary and secondary_path is None:
    raise SystemExit(
        "agent-generative-workloads: secondary report is required but not configured"
    )

def load_gauntlet(path: pathlib.Path) -> dict:
    if not path.exists():
        raise SystemExit(
            f"agent-generative-workloads: missing gauntlet report: {path}; "
            "run 'bash scripts/update_agent_reference_workflows_report.sh'"
        )
    doc = json.loads(path.read_text(encoding="utf-8"))
    if doc.get("kind") != "genesis/agent-capability-gauntlet-v0.1":
        raise SystemExit(
            f"agent-generative-workloads: unexpected report kind in {path}: {doc.get('kind')!r}"
        )
    workflows = doc.get("workflows")
    if not isinstance(workflows, list):
        raise SystemExit(f"agent-generative-workloads: malformed workflows array in {path}")
    by_name = {}
    for wf in workflows:
        if not isinstance(wf, dict):
            continue
        if not wf.get("ok", False):
            continue
        name = wf.get("name")
        duration = wf.get("duration_ms")
        domains = wf.get("domains")
        replay_hash = wf.get("replay_hash_normalized") or wf.get("replay_hash")
        if (
            isinstance(name, str)
            and isinstance(duration, int)
            and isinstance(domains, list)
            and all(isinstance(d, str) for d in domains)
            and isinstance(replay_hash, str)
        ):
            by_name[name] = {
                "duration_ms": duration,
                "domains": sorted(set(domains)),
                "replay_hash": replay_hash,
            }
    if not by_name:
        raise SystemExit(f"agent-generative-workloads: no successful workflows available in {path}")
    required_domains = doc.get("required_domains")
    if not isinstance(required_domains, list):
        required_domains = []
    required_domains = sorted(
        {
            domain
            for domain in required_domains
            if isinstance(domain, str) and domain
        }
    )
    return {
        "runtime_profile": str(doc.get("runtime_profile", "native")),
        "workflows": by_name,
        "elapsed_ms": int(doc.get("elapsed_ms", 0)),
        "required_domains": required_domains,
    }

primary = load_gauntlet(primary_path)
secondary = load_gauntlet(secondary_path) if secondary_path else None
if require_secondary and secondary is None:
    raise SystemExit("agent-generative-workloads: secondary report is required")

workflow_names = sorted(primary["workflows"].keys())
if secondary:
    workflow_names = sorted(set(workflow_names) & set(secondary["workflows"].keys()))

if len(workflow_names) < min_workflows:
    raise SystemExit(
        "agent-generative-workloads: insufficient workflow pool for generation: "
        f"{len(workflow_names)} < {min_workflows}"
    )

effective_max = min(max_workflows, len(workflow_names))
cases = []
used_signatures = set()

def mutate_selection(selection: list[str], mode: int, rng_obj: random.Random) -> list[str]:
    out = list(selection)
    if mode == 0:
        rng_obj.shuffle(out)
    elif mode == 1:
        if len(out) > 1:
            rotate = rng_obj.randint(1, len(out) - 1)
            out = out[rotate:] + out[:rotate]
    elif mode == 2:
        out = sorted(out)
    elif mode == 3:
        out = sorted(out, reverse=True)
    return out

for i in range(case_count):
    case_rng = random.Random(f"{seed}:{i}")
    k = case_rng.randint(min_workflows, effective_max)
    selection = case_rng.sample(workflow_names, k)
    selection = mutate_selection(selection, i % 4, case_rng)
    signature = "|".join(selection)
    if signature in used_signatures:
        selection = selection[1:] + selection[:1]
        signature = "|".join(selection)
    used_signatures.add(signature)
    case_id = f"mut-{i:02d}-{hashlib.sha256(signature.encode('utf-8')).hexdigest()[:10]}"
    cases.append({"id": case_id, "workflows": selection})

def evaluate_case(case: dict, wf_map: dict) -> dict:
    domains = []
    duration_ms = 0
    replay_components = []
    for name in case["workflows"]:
        wf = wf_map[name]
        duration_ms += int(wf["duration_ms"])
        domains.extend(wf["domains"])
        replay_components.append(wf["replay_hash"])
    domains_unique = sorted(set(domains))
    replay_digest = hashlib.sha256("|".join(replay_components).encode("utf-8")).hexdigest()
    return {
        "id": case["id"],
        "workflow_count": len(case["workflows"]),
        "workflows": case["workflows"],
        "domain_count": len(domains_unique),
        "domains": domains_unique,
        "duration_ms": duration_ms,
        "replay_digest": replay_digest,
        "duration_ok": duration_ms <= max_case_duration_ms,
        "domain_ok": len(domains_unique) >= min_domain_count,
    }

primary_cases = [evaluate_case(case, primary["workflows"]) for case in cases]
secondary_cases = [evaluate_case(case, secondary["workflows"]) for case in cases] if secondary else []
secondary_by_id = {case["id"]: case for case in secondary_cases}
required_domains = sorted(primary.get("required_domains") or [])
if not required_domains:
    required_domains = sorted(
        {
            domain
            for name in workflow_names
            for domain in primary["workflows"][name]["domains"]
        }
    )

baseline_history_rows = []
history_rows = []
history_candidates = [(baseline_history_file, True)]
for candidate in (history_input_file, history_file):
    if all(candidate != existing for existing, _ in history_candidates):
        history_candidates.append((candidate, False))
for candidate, is_baseline in history_candidates:
    if not candidate.exists():
        continue
    for raw in candidate.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line:
            continue
        try:
            row = json.loads(line)
        except json.JSONDecodeError:
            continue
        if not isinstance(row, dict):
            continue
        if row.get("kind") != "genesis/agent-generative-workloads-v0.1":
            continue
        if row.get("runtime_profile") != primary["runtime_profile"]:
            continue
        if row.get("seed") != seed:
            continue
        durations = row.get("case_durations_ms")
        if isinstance(durations, dict):
            if is_baseline:
                baseline_history_rows.append(durations)
            history_rows.append(durations)

if require_min_history and not baseline_history_file.exists():
    raise SystemExit(
        f"agent-generative-workloads: baseline history file missing: {baseline_history_file}"
    )

def p95(values: list[int]) -> int:
    ordered = sorted(values)
    idx = max(0, min(len(ordered) - 1, int(round(0.95 * (len(ordered) - 1)))))
    return ordered[idx]

case_regressions = []
for case in primary_cases:
    baseline_prior = []
    for row in baseline_history_rows:
        value = row.get(case["id"])
        if isinstance(value, int):
            baseline_prior.append(value)
    prior = []
    for row in history_rows:
        value = row.get(case["id"])
        if isinstance(value, int):
            prior.append(value)
    baseline_seeded = len(baseline_prior) >= p95_min_samples
    history_seeded = len(prior) >= p95_min_samples
    history_bootstrap_mode = require_min_history and (not history_seeded)
    case["history_samples"] = len(prior) + 1
    case["baseline_seeded"] = baseline_seeded
    case["history_bootstrap_mode"] = history_bootstrap_mode
    case["history_min_ok"] = history_seeded or (not require_min_history)
    case["history_p95_ms"] = p95(prior) if prior else None
    case["regression_enforced"] = history_seeded and bool(prior)
    case["regression_budget_ms"] = (
        int(math.ceil(case["history_p95_ms"] * (1.0 + regression_percent / 100.0)))
        + regression_slack_ms
        if case["regression_enforced"]
        else None
    )
    case["regression_ok"] = (
        True
        if case["regression_budget_ms"] is None
        else case["duration_ms"] <= case["regression_budget_ms"]
    )
    if require_min_history and not case["history_min_ok"]:
        case["regression_ok"] = False
    if not case["regression_ok"]:
        case_regressions.append(
            f"{case['id']} duration {case['duration_ms']} > regression budget {case['regression_budget_ms']}"
        )

parity_mismatches = []
if secondary:
    for case in primary_cases:
        other = secondary_by_id.get(case["id"])
        if other is None:
            parity_mismatches.append(f"{case['id']}:missing-secondary-case")
            continue
        case["secondary_runtime_profile"] = secondary["runtime_profile"]
        case["secondary_duration_ms"] = other["duration_ms"]
        case["secondary_replay_digest"] = other["replay_digest"]
        case["parity_ok"] = case["replay_digest"] == other["replay_digest"]
        if not case["parity_ok"]:
            parity_mismatches.append(f"{case['id']}:replay-digest-mismatch")
else:
    for case in primary_cases:
        case["parity_ok"] = True

duration_failures = [case["id"] for case in primary_cases if not case["duration_ok"]]
domain_failures = [case["id"] for case in primary_cases if not case["domain_ok"]]
regression_failures = [case["id"] for case in primary_cases if not case["regression_ok"]]
history_min_failures = [
    case["id"]
    for case in primary_cases
    if require_min_history and not case["history_min_ok"]
]
covered_domains = sorted({domain for case in primary_cases for domain in case["domains"]})
domain_coverage_failures = sorted(set(required_domains) - set(covered_domains))

ok = not (
    duration_failures
    or domain_failures
    or regression_failures
    or parity_mismatches
    or history_min_failures
    or domain_coverage_failures
)

durations = [case["duration_ms"] for case in primary_cases]
summary = {
    "case_count": len(primary_cases),
    "duration_min_ms": min(durations),
    "duration_median_ms": int(round(statistics.median(durations))),
    "duration_p95_ms": p95(durations),
    "duration_max_ms": max(durations),
}

timestamp = dt.datetime.now(dt.timezone.utc).replace(microsecond=0).isoformat()
report = {
    "kind": "genesis/agent-generative-workloads-v0.1",
    "ok": ok,
    "seed": seed,
    "runtime_profile": primary["runtime_profile"],
    "secondary_runtime_profile": secondary["runtime_profile"] if secondary else None,
    "primary_report": str(primary_path),
    "secondary_report": str(secondary_path) if secondary_path else None,
    "history_path": str(history_file),
    "history_input_path": str(history_input_file),
    "baseline_history_path": str(baseline_history_file),
    "require_min_history": require_min_history,
    "require_secondary": require_secondary,
    "case_count": len(primary_cases),
    "min_workflows": min_workflows,
    "max_workflows": max_workflows,
    "min_domain_count": min_domain_count,
    "max_case_duration_ms": max_case_duration_ms,
    "p95_min_samples": p95_min_samples,
    "regression_percent": regression_percent,
    "regression_slack_ms": regression_slack_ms,
    "summary": summary,
    "duration_failures": duration_failures,
    "domain_failures": domain_failures,
    "regression_failures": regression_failures,
    "history_min_failures": history_min_failures,
    "required_domains": required_domains,
    "covered_domains": covered_domains,
    "domain_coverage_failures": domain_coverage_failures,
    "parity_mismatches": parity_mismatches,
    "cases": primary_cases,
    "timestamp_utc": timestamp,
}

report_file.parent.mkdir(parents=True, exist_ok=True)
report_file.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")

history_file.parent.mkdir(parents=True, exist_ok=True)
history_entry = {
    "kind": report["kind"],
    "ok": ok,
    "seed": seed,
    "runtime_profile": primary["runtime_profile"],
    "secondary_runtime_profile": secondary["runtime_profile"] if secondary else None,
    "case_count": len(primary_cases),
    "case_durations_ms": {case["id"]: case["duration_ms"] for case in primary_cases},
    "case_replay_digests": {case["id"]: case["replay_digest"] for case in primary_cases},
    "duration_p95_ms": summary["duration_p95_ms"],
    "timestamp_utc": timestamp,
}
with history_file.open("a", encoding="utf-8") as fh:
    fh.write(json.dumps(history_entry, sort_keys=True) + "\n")

print(
    "agent-generative-workloads: "
    f"report={report_file} ok={ok} cases={len(primary_cases)} "
    f"duration_p95_ms={summary['duration_p95_ms']} parity_mismatches={len(parity_mismatches)}"
)

if not ok:
    reasons = []
    if duration_failures:
        reasons.append("duration-failures=" + ",".join(duration_failures))
    if domain_failures:
        reasons.append("domain-failures=" + ",".join(domain_failures))
    if regression_failures:
        reasons.append("regression-failures=" + ",".join(regression_failures))
    if history_min_failures:
        reasons.append("history-min-failures=" + ",".join(history_min_failures))
    if domain_coverage_failures:
        reasons.append("domain-coverage-failures=" + ",".join(domain_coverage_failures))
    if parity_mismatches:
        reasons.append("parity-mismatches=" + ",".join(parity_mismatches))
    raise SystemExit("agent-generative-workloads: " + "; ".join(reasons))
PY
