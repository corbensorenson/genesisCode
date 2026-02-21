#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

GENESIS_BIN="${GENESIS_BIN:-$ROOT_DIR/target/debug/genesis}"
if [[ ! -x "$GENESIS_BIN" ]]; then
  cargo build -p gc_cli >/dev/null
fi

GAUNTLET_REPORT="${GENESIS_AGENT_GAUNTLET_REPORT:-.genesis/perf/agent_capability_gauntlet_report.json}"
GAUNTLET_HISTORY="${GENESIS_AGENT_GAUNTLET_HISTORY:-.genesis/perf/agent_capability_gauntlet_history.jsonl}"
GAUNTLET_DEFAULT_MAX_MS="${GENESIS_AGENT_GAUNTLET_DEFAULT_MAX_MS:-300000}"

python3 - "$ROOT_DIR" "$GENESIS_BIN" "$GAUNTLET_REPORT" "$GAUNTLET_HISTORY" "$GAUNTLET_DEFAULT_MAX_MS" <<'PY'
import datetime as dt
import json
import os
import pathlib
import subprocess
import sys
import time

root = pathlib.Path(sys.argv[1])
genesis_bin = sys.argv[2]
report_path = pathlib.Path(sys.argv[3])
history_path = pathlib.Path(sys.argv[4])
default_max_ms = int(sys.argv[5])

workflows = [
    {
        "name": "agent_compute_workflow",
        "path": "examples/agent_compute_workflow/workflow.sh",
        "domains": ["gpu_compute"],
    },
    {
        "name": "agent_gpu_compute_workflow",
        "path": "examples/agent_gpu_compute_workflow/workflow.sh",
        "domains": ["gpu_compute"],
    },
    {
        "name": "agent_interactive_gfx_compute_workflow",
        "path": "examples/agent_interactive_gfx_compute_workflow/workflow.sh",
        "domains": ["graphics", "gpu_compute"],
    },
    {
        "name": "agent_long_running_gfx_loop_workflow",
        "path": "examples/agent_long_running_gfx_loop_workflow/workflow.sh",
        "domains": ["graphics"],
    },
    {
        "name": "agent_service_workflow",
        "path": "examples/agent_service_workflow/workflow.sh",
        "domains": ["service", "package_publish_sync"],
    },
    {
        "name": "agent_network_process_workflow",
        "path": "examples/agent_network_process_workflow/workflow.sh",
        "domains": ["network_process", "service"],
    },
    {
        "name": "agent_multi_package_publish_workflow",
        "path": "examples/agent_multi_package_publish_workflow/workflow.sh",
        "domains": ["package_publish_sync"],
    },
]

required_domains = {
    "service": 1,
    "network_process": 1,
    "package_publish_sync": 1,
    "graphics": 1,
    "gpu_compute": 1,
}

env = dict(os.environ)
env["GENESIS_BIN"] = genesis_bin

started = time.time()
workflow_reports = []

for wf in workflows:
    max_ms = int(env.get(f"GENESIS_AGENT_GAUNTLET_MAX_MS_{wf['name'].upper()}", default_max_ms))
    cmd = ["bash", str(root / wf["path"])]
    print(f"agent-capability-gauntlet: running {wf['name']}")
    wf_start = time.time()
    proc = subprocess.run(
        cmd,
        cwd=root,
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    duration_ms = int((time.time() - wf_start) * 1000)
    combined = (proc.stdout or "") + (proc.stderr or "")
    replay_signal = "replay=" in combined
    duration_ok = duration_ms <= max_ms
    exit_ok = proc.returncode == 0
    ok = exit_ok and replay_signal and duration_ok
    workflow_reports.append(
        {
            "name": wf["name"],
            "path": wf["path"],
            "domains": sorted(wf["domains"]),
            "exit_code": proc.returncode,
            "exit_ok": exit_ok,
            "replay_signal": replay_signal,
            "duration_ms": duration_ms,
            "max_ms": max_ms,
            "duration_ok": duration_ok,
            "ok": ok,
            "stdout_tail": (proc.stdout or "")[-400:],
            "stderr_tail": (proc.stderr or "")[-400:],
        }
    )

workflow_reports.sort(key=lambda x: x["name"])
domain_reports = []
for domain, required_successes in sorted(required_domains.items()):
    successes = sum(1 for wf in workflow_reports if wf["ok"] and domain in wf["domains"])
    domain_reports.append(
        {
            "domain": domain,
            "required_successes": required_successes,
            "successes": successes,
            "ok": successes >= required_successes,
        }
    )

workflow_count = len(workflow_reports)
workflow_successes = sum(1 for wf in workflow_reports if wf["ok"])
score_percent = round((workflow_successes / workflow_count) * 100.0, 2) if workflow_count else 0.0
domain_ok = all(d["ok"] for d in domain_reports)
all_workflows_ok = workflow_successes == workflow_count
ok = domain_ok and all_workflows_ok

report = {
    "kind": "genesis/agent-capability-gauntlet-v0.1",
    "ok": ok,
    "workflow_count": workflow_count,
    "workflow_successes": workflow_successes,
    "score_percent": score_percent,
    "domain_count": len(domain_reports),
    "domain_successes": sum(1 for d in domain_reports if d["ok"]),
    "elapsed_ms": int((time.time() - started) * 1000),
    "default_max_ms": default_max_ms,
    "domains": domain_reports,
    "workflows": workflow_reports,
    "timestamp_utc": dt.datetime.now(dt.timezone.utc).replace(microsecond=0).isoformat(),
}

report_path.parent.mkdir(parents=True, exist_ok=True)
report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")

history_path.parent.mkdir(parents=True, exist_ok=True)
history_entry = {
    "kind": report["kind"],
    "ok": ok,
    "score_percent": score_percent,
    "workflow_successes": workflow_successes,
    "workflow_count": workflow_count,
    "domain_successes": report["domain_successes"],
    "domain_count": report["domain_count"],
    "elapsed_ms": report["elapsed_ms"],
    "timestamp_utc": report["timestamp_utc"],
}
with history_path.open("a", encoding="utf-8") as f:
    f.write(json.dumps(history_entry, sort_keys=True) + "\n")

print(
    f"agent-capability-gauntlet: report={report_path} "
    f"score={score_percent}% workflows={workflow_successes}/{workflow_count} "
    f"domains={report['domain_successes']}/{report['domain_count']}"
)

if not ok:
    failing_workflows = [wf["name"] for wf in workflow_reports if not wf["ok"]]
    failing_domains = [d["domain"] for d in domain_reports if not d["ok"]]
    if failing_workflows:
        print(
            "agent-capability-gauntlet: failing workflows: "
            + ", ".join(failing_workflows),
            file=sys.stderr,
        )
    if failing_domains:
        print(
            "agent-capability-gauntlet: failing domains: "
            + ", ".join(failing_domains),
            file=sys.stderr,
        )
    sys.exit(1)
PY
