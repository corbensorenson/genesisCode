#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

GAUNTLET_REPORT="${GENESIS_AGENT_GAUNTLET_REPORT:-.genesis/perf/agent_capability_gauntlet_report.json}"
GAUNTLET_HISTORY="${GENESIS_AGENT_GAUNTLET_HISTORY:-.genesis/perf/agent_capability_gauntlet_history.jsonl}"
SCENARIO_REPORT="${GENESIS_AGENT_SCENARIO_REPORT:-.genesis/perf/agent_scenario_perf_report.json}"
SCENARIO_HISTORY="${GENESIS_AGENT_SCENARIO_HISTORY:-.genesis/perf/agent_scenario_perf_history.jsonl}"
SCENARIO_NAME="${GENESIS_AGENT_SCENARIO_NAME:-service-data-gfx-network}"
REQUIRED_WORKFLOWS_CSV="${GENESIS_AGENT_SCENARIO_WORKFLOWS:-agent_service_workflow,agent_durable_data_workflow,agent_long_running_gfx_loop_workflow,agent_network_process_workflow}"
MEDIAN_BUDGET_MS="${GENESIS_AGENT_SCENARIO_MEDIAN_BUDGET_MS:-600000}"
P95_BUDGET_MS="${GENESIS_AGENT_SCENARIO_P95_BUDGET_MS:-750000}"
P95_MIN_SAMPLES="${GENESIS_AGENT_SCENARIO_P95_MIN_SAMPLES:-8}"
REGRESSION_PERCENT="${GENESIS_AGENT_SCENARIO_REGRESSION_PERCENT:-25}"
CONTENTION_WARN_PERCENT="${GENESIS_AGENT_SCENARIO_CONTENTION_WARN_PERCENT:-50}"

python3 - \
  "$GAUNTLET_REPORT" \
  "$GAUNTLET_HISTORY" \
  "$SCENARIO_REPORT" \
  "$SCENARIO_HISTORY" \
  "$SCENARIO_NAME" \
  "$REQUIRED_WORKFLOWS_CSV" \
  "$MEDIAN_BUDGET_MS" \
  "$P95_BUDGET_MS" \
  "$P95_MIN_SAMPLES" \
  "$REGRESSION_PERCENT" \
  "$CONTENTION_WARN_PERCENT" <<'PY'
import datetime as dt
import json
import math
import pathlib
import statistics
import sys

(
    gauntlet_report_path,
    gauntlet_history_path,
    scenario_report_path,
    scenario_history_path,
    scenario_name,
    required_workflows_csv,
    median_budget_ms,
    p95_budget_ms,
    p95_min_samples,
    regression_percent,
    contention_warn_percent,
) = sys.argv[1:]

required_workflows = [x.strip() for x in required_workflows_csv.split(",") if x.strip()]
if not required_workflows:
    raise SystemExit("agent-scenario-perf: required workflow list is empty")

median_budget_ms = int(median_budget_ms)
p95_budget_ms = int(p95_budget_ms)
p95_min_samples = int(p95_min_samples)
regression_percent = float(regression_percent)
contention_warn_percent = float(contention_warn_percent)
if median_budget_ms <= 0 or p95_budget_ms <= 0:
    raise SystemExit("agent-scenario-perf: median/p95 budgets must be positive integers")
if p95_min_samples <= 0:
    raise SystemExit("agent-scenario-perf: p95 min samples must be >= 1")
if regression_percent < 0:
    raise SystemExit("agent-scenario-perf: regression percent must be >= 0")
if contention_warn_percent < 0:
    raise SystemExit("agent-scenario-perf: contention warn percent must be >= 0")


def p95(values: list[int]) -> int:
    if not values:
        return 0
    ordered = sorted(values)
    idx = int(round(0.95 * (len(ordered) - 1)))
    return ordered[idx]


def median(values: list[int]) -> int:
    if not values:
        return 0
    return int(round(statistics.median(values)))


gauntlet_report_file = pathlib.Path(gauntlet_report_path)
if not gauntlet_report_file.exists():
    raise SystemExit(
        f"agent-scenario-perf: gauntlet report missing at {gauntlet_report_file}; run scripts/check_agent_reference_workflows.sh first"
    )
report_doc = json.loads(gauntlet_report_file.read_text(encoding="utf-8"))
if report_doc.get("kind") != "genesis/agent-capability-gauntlet-v0.1":
    raise SystemExit(
        f"agent-scenario-perf: unexpected gauntlet report kind {report_doc.get('kind')!r}"
    )
if not isinstance(report_doc.get("workflows"), list):
    raise SystemExit("agent-scenario-perf: gauntlet report missing workflows array")

runtime_profile = report_doc.get("runtime_profile", "native")
report_timestamp = report_doc.get("timestamp_utc")

component_durations: dict[str, int] = {}
component_ok: dict[str, bool] = {}
for wf in report_doc["workflows"]:
    if not isinstance(wf, dict):
        continue
    name = wf.get("name")
    duration = wf.get("duration_ms")
    if isinstance(name, str) and isinstance(duration, int):
        component_durations[name] = duration
        component_ok[name] = bool(wf.get("ok", False))

missing = [name for name in required_workflows if name not in component_durations]
if missing:
    raise SystemExit(
        "agent-scenario-perf: missing scenario workflow durations in gauntlet report: "
        + ", ".join(missing)
    )

failing_components = [name for name in required_workflows if not component_ok.get(name, False)]
if failing_components:
    raise SystemExit(
        "agent-scenario-perf: required scenario workflows failed in gauntlet run: "
        + ", ".join(failing_components)
    )

current_duration_ms = sum(component_durations[name] for name in required_workflows)

history_samples: list[int] = []
gauntlet_history_file = pathlib.Path(gauntlet_history_path)
if gauntlet_history_file.exists():
    for line in gauntlet_history_file.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            entry = json.loads(line)
        except json.JSONDecodeError:
            continue
        if entry.get("kind") != "genesis/agent-capability-gauntlet-v0.1":
            continue
        if entry.get("runtime_profile") != runtime_profile:
            continue
        if report_timestamp is not None and entry.get("timestamp_utc") == report_timestamp:
            continue
        durations = entry.get("workflow_durations_ms")
        if not isinstance(durations, dict):
            continue
        if any(not isinstance(durations.get(name), int) for name in required_workflows):
            continue
        history_samples.append(sum(int(durations[name]) for name in required_workflows))

samples_ms = history_samples + [current_duration_ms]
sample_count = len(samples_ms)
median_ms = median(samples_ms)
p95_ms = p95(samples_ms)
baseline_p95_ms = p95(history_samples) if history_samples else None

median_ok = median_ms <= median_budget_ms
p95_enforced = sample_count >= p95_min_samples
p95_ok = (not p95_enforced) or (p95_ms <= p95_budget_ms)

regression_enforced = baseline_p95_ms is not None and len(history_samples) >= p95_min_samples
regression_budget_ms = None
regression_ok = True
if regression_enforced and baseline_p95_ms is not None:
    regression_budget_ms = int(math.ceil(baseline_p95_ms * (1.0 + regression_percent / 100.0)))
    regression_ok = current_duration_ms <= regression_budget_ms

spread_percent = 0.0
if sample_count > 1 and median_ms > 0:
    spread_percent = ((max(samples_ms) - min(samples_ms)) / median_ms) * 100.0
contention_warning = spread_percent >= contention_warn_percent

ok = median_ok and p95_ok and regression_ok

scenario_report = {
    "kind": "genesis/agent-scenario-perf-v0.1",
    "ok": ok,
    "scenario_name": scenario_name,
    "runtime_profile": runtime_profile,
    "required_workflows": required_workflows,
    "component_durations_ms": {name: component_durations[name] for name in required_workflows},
    "scenario_duration_ms": current_duration_ms,
    "sample_count": sample_count,
    "samples_ms": samples_ms,
    "median_ms": median_ms,
    "median_budget_ms": median_budget_ms,
    "median_ok": median_ok,
    "p95_ms": p95_ms,
    "p95_budget_ms": p95_budget_ms,
    "p95_min_samples": p95_min_samples,
    "p95_enforced": p95_enforced,
    "p95_ok": p95_ok,
    "baseline_p95_ms": baseline_p95_ms,
    "regression_percent": regression_percent,
    "regression_enforced": regression_enforced,
    "regression_budget_ms": regression_budget_ms,
    "regression_ok": regression_ok,
    "contention_warn_percent": contention_warn_percent,
    "contention_spread_percent": round(spread_percent, 2),
    "contention_warning": contention_warning,
    "gauntlet_report": str(gauntlet_report_file),
    "gauntlet_history": str(gauntlet_history_file),
    "timestamp_utc": dt.datetime.now(dt.timezone.utc).replace(microsecond=0).isoformat(),
}

scenario_report_file = pathlib.Path(scenario_report_path)
scenario_report_file.parent.mkdir(parents=True, exist_ok=True)
scenario_report_file.write_text(
    json.dumps(scenario_report, indent=2, sort_keys=True) + "\n",
    encoding="utf-8",
)

history_entry = {
    "kind": scenario_report["kind"],
    "ok": ok,
    "scenario_name": scenario_name,
    "runtime_profile": runtime_profile,
    "scenario_duration_ms": current_duration_ms,
    "median_ms": median_ms,
    "p95_ms": p95_ms,
    "sample_count": sample_count,
    "timestamp_utc": scenario_report["timestamp_utc"],
}
scenario_history_file = pathlib.Path(scenario_history_path)
scenario_history_file.parent.mkdir(parents=True, exist_ok=True)
with scenario_history_file.open("a", encoding="utf-8") as f:
    f.write(json.dumps(history_entry, sort_keys=True) + "\n")

print(
    "agent-scenario-perf: "
    f"scenario={scenario_name} "
    f"runtime_profile={runtime_profile} "
    f"current_ms={current_duration_ms} median_ms={median_ms}/{median_budget_ms} "
    f"p95_ms={p95_ms}/{p95_budget_ms} samples={sample_count} "
    f"report={scenario_report_file}"
)
if contention_warning:
    print(
        "agent-scenario-perf: contention warning "
        f"spread={spread_percent:.2f}% threshold={contention_warn_percent:.2f}%"
    )

if not ok:
    reasons = []
    if not median_ok:
        reasons.append(f"median {median_ms} > budget {median_budget_ms}")
    if not p95_ok:
        reasons.append(f"p95 {p95_ms} > budget {p95_budget_ms}")
    if not regression_ok:
        reasons.append(
            f"current {current_duration_ms} > regression budget {regression_budget_ms} (baseline p95 {baseline_p95_ms})"
        )
    raise SystemExit("agent-scenario-perf: budget failure: " + "; ".join(reasons))
PY
