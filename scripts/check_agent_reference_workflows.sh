#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
source "$ROOT_DIR/scripts/lib/heavy_gate_preflight.sh"
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "check-agent-reference-workflows" \
  ".genesis/build/cargo" \
  "GENESIS_CHECK_AGENT_REFERENCE_WORKFLOWS_CARGO_TARGET_DIR"

DISK_MIN_FREE_KB="${GENESIS_AGENT_REFERENCE_WORKFLOWS_MIN_FREE_KB:-3145728}"
DISK_AUTO_RECLAIM="${GENESIS_AGENT_REFERENCE_WORKFLOWS_DISK_AUTO_RECLAIM:-1}"
TMP_ROOT="${GENESIS_AGENT_REFERENCE_WORKFLOWS_TMPDIR:-$ROOT_DIR/.genesis/tmp/check-agent-reference-workflows}"
genesis_heavy_gate_preflight \
  "$ROOT_DIR" \
  "agent-capability-gauntlet" \
  "$DISK_MIN_FREE_KB" \
  "$TMP_ROOT" \
  "$DISK_AUTO_RECLAIM"

DEFAULT_DEBUG_DIR="$CARGO_TARGET_DIR/debug"
GENESIS_BIN="${GENESIS_BIN:-$DEFAULT_DEBUG_DIR/genesis}"
if [[ ! -x "$GENESIS_BIN" ]]; then
  case "$(basename "$GENESIS_BIN")" in
    genesis_wasi|genesis_wasi_parity)
      cargo build -p gc_wasi_cli >/dev/null
      ;;
    *)
      cargo build -p gc_cli >/dev/null
      ;;
  esac
fi

GAUNTLET_REPORT="${GENESIS_AGENT_GAUNTLET_REPORT:-.genesis/perf/agent_capability_gauntlet_report.json}"
GAUNTLET_HISTORY="${GENESIS_AGENT_GAUNTLET_HISTORY:-.genesis/perf/agent_capability_gauntlet_history.jsonl}"
GAUNTLET_BASELINE_HISTORY="${GENESIS_AGENT_GAUNTLET_BASELINE_HISTORY:-policies/perf/agent_capability_gauntlet_seed_history.jsonl}"
GAUNTLET_DEFAULT_MAX_MS="${GENESIS_AGENT_GAUNTLET_DEFAULT_MAX_MS:-300000}"
GAUNTLET_REQUIRE_MIN_HISTORY="${GENESIS_AGENT_GAUNTLET_REQUIRE_MIN_HISTORY:-1}"
GAUNTLET_REGRESSION_PERCENT="${GENESIS_AGENT_GAUNTLET_REGRESSION_PERCENT:-25}"

python3 - \
  "$ROOT_DIR" \
  "$GENESIS_BIN" \
  "$GAUNTLET_REPORT" \
  "$GAUNTLET_HISTORY" \
  "$GAUNTLET_BASELINE_HISTORY" \
  "$GAUNTLET_DEFAULT_MAX_MS" \
  "$GAUNTLET_REQUIRE_MIN_HISTORY" \
  "$GAUNTLET_REGRESSION_PERCENT" <<'PY'
import datetime as dt
import hashlib
import json
import math
import os
import pathlib
import re
import subprocess
import sys
import time
from typing import Optional

root = pathlib.Path(sys.argv[1])
genesis_bin = sys.argv[2]
report_path = pathlib.Path(sys.argv[3])
history_path = pathlib.Path(sys.argv[4])
baseline_history_path = pathlib.Path(sys.argv[5])
default_max_ms = int(sys.argv[6])
require_min_history = sys.argv[7].strip().lower() not in {"0", "false", "no", "off"}
regression_percent = float(sys.argv[8])
regression_slack_ms = int(
    os.environ.get("GENESIS_AGENT_GAUNTLET_REGRESSION_SLACK_MS", "500")
)

workflows = [
    {
        "name": "agent_filesystem_workflow",
        "path": "examples/agent_filesystem_workflow/workflow.sh",
        "domains": ["filesystem"],
    },
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
        "name": "agent_browser_runtime_workflow",
        "path": "examples/agent_browser_runtime_workflow/workflow.sh",
        "domains": ["browser_runtime"],
    },
    {
        "name": "agent_xr_runtime_workflow",
        "path": "examples/agent_xr_runtime_workflow/workflow.sh",
        "domains": ["xr_runtime"],
    },
    {
        "name": "agent_deploy_bundle_workflow",
        "path": "examples/agent_deploy_bundle_workflow/workflow.sh",
        "domains": ["deployment"],
    },
    {
        "name": "agent_deploy_ios_workflow",
        "path": "examples/agent_deploy_ios_workflow/workflow.sh",
        "domains": ["deployment", "deploy_ios"],
    },
    {
        "name": "agent_deploy_android_workflow",
        "path": "examples/agent_deploy_android_workflow/workflow.sh",
        "domains": ["deployment", "deploy_android"],
    },
    {
        "name": "agent_deploy_edge_workflow",
        "path": "examples/agent_deploy_edge_workflow/workflow.sh",
        "domains": ["deployment", "deploy_edge"],
    },
    {
        "name": "agent_deploy_service_runtime_workflow",
        "path": "examples/agent_deploy_service_runtime_workflow/workflow.sh",
        "domains": ["deployment", "deploy_service_runtime"],
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
        "name": "agent_raw_network_sockets_workflow",
        "path": "examples/agent_raw_network_sockets_workflow/workflow.sh",
        "domains": ["raw_network_sockets"],
    },
    {
        "name": "agent_inbound_server_workflow",
        "path": "examples/agent_inbound_server_workflow/workflow.sh",
        "domains": ["inbound_server"],
    },
    {
        "name": "agent_durable_data_workflow",
        "path": "examples/agent_durable_data_workflow/workflow.sh",
        "domains": ["durable_data"],
    },
    {
        "name": "agent_process_lifecycle_workflow",
        "path": "examples/agent_process_lifecycle_workflow/workflow.sh",
        "domains": ["process_lifecycle"],
    },
    {
        "name": "agent_plugin_runtime_workflow",
        "path": "examples/agent_plugin_runtime_workflow/workflow.sh",
        "domains": ["plugin_runtime"],
    },
    {
        "name": "agent_time_control_workflow",
        "path": "examples/agent_time_control_workflow/workflow.sh",
        "domains": ["time_control"],
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
    "filesystem": 1,
    "raw_network_sockets": 1,
    "inbound_server": 1,
    "durable_data": 1,
    "process_lifecycle": 1,
    "plugin_runtime": 1,
    "time_control": 1,
    "browser_runtime": 1,
    "xr_runtime": 1,
    "deployment": 1,
    "deploy_ios": 1,
    "deploy_android": 1,
    "deploy_edge": 1,
    "deploy_service_runtime": 1,
}

env = dict(os.environ)
env["GENESIS_BIN"] = genesis_bin
profile = env.get("GENESIS_AGENT_GAUNTLET_PROFILE", "dev-fast").strip().lower()
runtime_profile = env.get("GENESIS_AGENT_GAUNTLET_RUNTIME_PROFILE", "native").strip().lower()
require_gpu_device_raw = env.get("GENESIS_AGENT_GAUNTLET_REQUIRE_GPU_DEVICE_BACKEND")
if require_gpu_device_raw is None:
    require_gpu_device = profile in {"release-full", "release"}
else:
    require_gpu_device = require_gpu_device_raw.strip().lower() in {"1", "true", "yes", "on"}
if require_gpu_device:
    env["GENESIS_AGENT_GPU_REQUIRE_DEVICE"] = "1"
p95_default_max_ms = int(
    env.get("GENESIS_AGENT_GAUNTLET_P95_DEFAULT_MAX_MS", str(default_max_ms))
)
p95_min_samples = int(env.get("GENESIS_AGENT_GAUNTLET_P95_MIN_SAMPLES", "8"))
if p95_min_samples < 1:
    raise SystemExit("agent-capability-gauntlet: GENESIS_AGENT_GAUNTLET_P95_MIN_SAMPLES must be >= 1")
if regression_percent < 0:
    raise SystemExit("agent-capability-gauntlet: GENESIS_AGENT_GAUNTLET_REGRESSION_PERCENT must be >= 0")
if regression_slack_ms < 0:
    raise SystemExit(
        "agent-capability-gauntlet: GENESIS_AGENT_GAUNTLET_REGRESSION_SLACK_MS must be >= 0"
    )

backend_pattern = re.compile(r':backend\s+"([^"]+)"')
replay_pattern = re.compile(r"replay=([^\r\n]+)")
compile_activity_pattern = re.compile(
    r"(^|\n)\s*Compiling\s+|Finished `(?:dev|release|selfhost-strict)` profile",
    re.MULTILINE,
)

def normalize_replay_value(workflow_name: str, replay_value: Optional[str]) -> Optional[str]:
    if replay_value is None:
        return None
    normalized = replay_value
    if workflow_name == "agent_time_control_workflow":
        normalized = re.sub(r":delta-ms\s+\d+", ":delta-ms <normalized>", normalized)
    return normalized

def p95(values: list[int]) -> int:
    if not values:
        return 0
    sorted_values = sorted(values)
    index = int(round(0.95 * (len(sorted_values) - 1)))
    return sorted_values[index]

def load_workflow_duration_history(
    paths: list[pathlib.Path], current_runtime_profile: str
) -> dict[str, list[int]]:
    durations: dict[str, list[int]] = {}
    for path in paths:
        if not path.exists():
            continue
        for raw_line in path.read_text(encoding="utf-8").splitlines():
            line = raw_line.strip()
            if not line:
                continue
            try:
                entry = json.loads(line)
            except json.JSONDecodeError:
                continue
            if entry.get("runtime_profile") != current_runtime_profile:
                continue
            wf_durations = entry.get("workflow_durations_ms")
            if not isinstance(wf_durations, dict):
                continue
            for name, value in wf_durations.items():
                if not isinstance(name, str) or not isinstance(value, int):
                    continue
                durations.setdefault(name, []).append(value)
    return durations

def load_elapsed_history(
    paths: list[pathlib.Path], current_runtime_profile: str
) -> list[int]:
    samples: list[int] = []
    for path in paths:
        if not path.exists():
            continue
        for raw_line in path.read_text(encoding="utf-8").splitlines():
            line = raw_line.strip()
            if not line:
                continue
            try:
                entry = json.loads(line)
            except json.JSONDecodeError:
                continue
            if entry.get("runtime_profile") != current_runtime_profile:
                continue
            elapsed = entry.get("elapsed_ms")
            if isinstance(elapsed, int) and elapsed > 0:
                samples.append(elapsed)
    return samples

started = time.time()
workflow_reports = []
if require_min_history and not baseline_history_path.exists():
    raise SystemExit(
        f"agent-capability-gauntlet: baseline history file missing: {baseline_history_path}"
    )
history_durations = load_workflow_duration_history(
    [baseline_history_path, history_path], runtime_profile
)
baseline_history_durations = load_workflow_duration_history(
    [baseline_history_path], runtime_profile
)

for wf in workflows:
    max_ms = int(env.get(f"GENESIS_AGENT_GAUNTLET_MAX_MS_{wf['name'].upper()}", default_max_ms))
    p95_budget_ms = int(
        env.get(
            f"GENESIS_AGENT_GAUNTLET_P95_MAX_MS_{wf['name'].upper()}",
            str(p95_default_max_ms),
        )
    )
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
    backend_matches = backend_pattern.findall(combined)
    reported_backend = backend_matches[-1] if backend_matches else None
    replay_matches = replay_pattern.findall(combined)
    replay_value = replay_matches[-1] if replay_matches else None
    replay_value_normalized = normalize_replay_value(wf["name"], replay_value)
    replay_hash = hashlib.sha256(replay_value.encode("utf-8")).hexdigest() if replay_value else None
    replay_hash_normalized = (
        hashlib.sha256(replay_value_normalized.encode("utf-8")).hexdigest()
        if replay_value_normalized
        else None
    )
    stderr_text = proc.stderr or ""
    build_bootstrap_mode = compile_activity_pattern.search(stderr_text) is not None
    gpu_backend_required = require_gpu_device and ("gpu_compute" in wf["domains"])
    gpu_backend_ok = (not gpu_backend_required) or (reported_backend == "device-runtime")
    replay_signal = replay_value is not None
    duration_ok = duration_ms <= max_ms
    wf_history_samples = history_durations.get(wf["name"], [])
    wf_samples = wf_history_samples + ([] if build_bootstrap_mode else [duration_ms])
    workflow_seeded = len(baseline_history_durations.get(wf["name"], [])) >= p95_min_samples
    history_bootstrap_mode = require_min_history and (not workflow_seeded)
    history_min_ok = (
        len(wf_samples) >= p95_min_samples
        or (not require_min_history)
        or history_bootstrap_mode
    )
    p95_duration_ms = p95(wf_samples)
    p95_enforced = len(wf_samples) >= p95_min_samples
    p95_ok = (not p95_enforced) or (p95_duration_ms <= p95_budget_ms)
    baseline_p95_ms = p95(wf_history_samples) if wf_history_samples else None
    regression_enforced = (
        (not build_bootstrap_mode)
        and baseline_p95_ms is not None
        and len(wf_history_samples) >= p95_min_samples
    )
    regression_budget_ms = (
        max(
            int(math.ceil(baseline_p95_ms * (1.0 + regression_percent / 100.0))),
            int(baseline_p95_ms + regression_slack_ms),
        )
        if regression_enforced and baseline_p95_ms is not None
        else None
    )
    regression_ok = (
        True if regression_budget_ms is None else p95_duration_ms <= regression_budget_ms
    )
    exit_ok = proc.returncode == 0
    ok = (
        exit_ok
        and replay_signal
        and duration_ok
        and gpu_backend_ok
        and p95_ok
        and regression_ok
        and (history_min_ok or (not require_min_history))
    )
    workflow_reports.append(
        {
            "name": wf["name"],
            "path": wf["path"],
            "domains": sorted(wf["domains"]),
            "exit_code": proc.returncode,
            "exit_ok": exit_ok,
            "replay_signal": replay_signal,
            "replay_value": replay_value,
            "replay_value_normalized": replay_value_normalized,
            "replay_hash": replay_hash,
            "replay_hash_normalized": replay_hash_normalized,
            "duration_ms": duration_ms,
            "max_ms": max_ms,
            "duration_ok": duration_ok,
            "p95_budget_ms": p95_budget_ms,
            "p95_duration_ms": p95_duration_ms,
            "p95_enforced": p95_enforced,
            "p95_min_samples": p95_min_samples,
            "p95_sample_count": len(wf_samples),
            "p95_ok": p95_ok,
            "history_min_ok": history_min_ok,
            "history_bootstrap_mode": history_bootstrap_mode,
            "build_bootstrap_mode": build_bootstrap_mode,
            "require_min_history": require_min_history,
            "baseline_history_sample_count": len(wf_history_samples),
            "baseline_p95_ms": baseline_p95_ms,
            "regression_percent": regression_percent,
            "regression_slack_ms": regression_slack_ms,
            "regression_enforced": regression_enforced,
            "regression_budget_ms": regression_budget_ms,
            "regression_observed_ms": p95_duration_ms,
            "regression_ok": regression_ok,
            "gpu_backend_required": gpu_backend_required,
            "gpu_backend": reported_backend,
            "gpu_backend_ok": gpu_backend_ok,
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
p95_failures = [wf["name"] for wf in workflow_reports if not wf["p95_ok"]]
regression_failures = [wf["name"] for wf in workflow_reports if not wf["regression_ok"]]
history_min_failures = [
    wf["name"]
    for wf in workflow_reports
    if wf["require_min_history"] and not wf["history_min_ok"]
]
elapsed_ms = int((time.time() - started) * 1000)
budget_ms = default_max_ms
elapsed_history_samples = load_elapsed_history(
    [baseline_history_path, history_path], runtime_profile
)
elapsed_samples = elapsed_history_samples + [elapsed_ms]
history_samples = len(elapsed_samples)
history_p95_ms = p95(elapsed_samples)
history_p95_enforced = history_samples >= p95_min_samples
history_p95_ok = (not history_p95_enforced) or (history_p95_ms <= budget_ms)
elapsed_budget_ok = elapsed_ms <= budget_ms
history_min_ok_global = (not require_min_history) or (history_samples >= p95_min_samples)
ok = (
    domain_ok
    and all_workflows_ok
    and elapsed_budget_ok
    and history_p95_ok
    and history_min_ok_global
)
fail_reasons = []
if not all_workflows_ok:
    fail_reasons.append("workflow-failures")
if not domain_ok:
    fail_reasons.append("domain-coverage")
if not elapsed_budget_ok:
    fail_reasons.append("elapsed-budget")
if not history_p95_ok:
    fail_reasons.append("history-p95-budget")
if not history_min_ok_global:
    fail_reasons.append("insufficient-history")
if p95_failures:
    fail_reasons.append("workflow-p95-budget")
if regression_failures:
    fail_reasons.append("workflow-regression-budget")
if history_min_failures:
    fail_reasons.append("workflow-insufficient-history")

report = {
    "kind": "genesis/agent-capability-gauntlet-v0.1",
    "ok": ok,
    "workflow_count": workflow_count,
    "workflow_successes": workflow_successes,
    "score_percent": score_percent,
    "domain_count": len(domain_reports),
    "domain_successes": sum(1 for d in domain_reports if d["ok"]),
    "elapsed_ms": elapsed_ms,
    "budget_ms": budget_ms,
    "history_samples": history_samples,
    "history_p95_ms": history_p95_ms,
    "history_p95_enforced": history_p95_enforced,
    "history_p95_ok": history_p95_ok,
    "history_min_ok": history_min_ok_global,
    "fail_reasons": fail_reasons,
    "default_max_ms": default_max_ms,
    "p95_default_max_ms": p95_default_max_ms,
    "p95_min_samples": p95_min_samples,
    "p95_failures": p95_failures,
    "baseline_history_path": str(baseline_history_path),
    "require_min_history": require_min_history,
    "regression_percent": regression_percent,
    "regression_slack_ms": regression_slack_ms,
    "regression_failures": regression_failures,
    "history_min_failures": history_min_failures,
    "profile": profile,
    "runtime_profile": runtime_profile,
    "require_gpu_device_backend": require_gpu_device,
    "genesis_bin": genesis_bin,
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
    "budget_ms": report["budget_ms"],
    "history_p95_ms": report["history_p95_ms"],
    "runtime_profile": runtime_profile,
    "workflow_durations_ms": {
        wf["name"]: wf["duration_ms"]
        for wf in workflow_reports
        if not wf.get("build_bootstrap_mode")
    },
    "timestamp_utc": report["timestamp_utc"],
}
with history_path.open("a", encoding="utf-8") as f:
    f.write(json.dumps(history_entry, sort_keys=True) + "\n")

print(
    f"agent-capability-gauntlet: report={report_path} "
    f"runtime_profile={runtime_profile} "
    f"score={score_percent}% workflows={workflow_successes}/{workflow_count} "
    f"domains={report['domain_successes']}/{report['domain_count']} "
    f"elapsed_ms={elapsed_ms} budget_ms={budget_ms} history_p95_ms={history_p95_ms}"
)

if not ok:
    failing_workflows = [wf["name"] for wf in workflow_reports if not wf["ok"]]
    failing_gpu_backend = [
        wf["name"] for wf in workflow_reports if wf.get("gpu_backend_required") and not wf.get("gpu_backend_ok")
    ]
    failing_p95 = [wf["name"] for wf in workflow_reports if not wf.get("p95_ok")]
    failing_regression = [wf["name"] for wf in workflow_reports if not wf.get("regression_ok")]
    failing_history_min = [
        wf["name"]
        for wf in workflow_reports
        if wf.get("require_min_history") and not wf.get("history_min_ok")
    ]
    failing_domains = [d["domain"] for d in domain_reports if not d["ok"]]
    if fail_reasons:
        print(
            "agent-capability-gauntlet: fail reasons: "
            + ", ".join(fail_reasons),
            file=sys.stderr,
        )
    if failing_workflows:
        print(
            "agent-capability-gauntlet: failing workflows: "
            + ", ".join(failing_workflows),
            file=sys.stderr,
        )
    if failing_gpu_backend:
        print(
            "agent-capability-gauntlet: gpu backend contract failures: "
            + ", ".join(failing_gpu_backend),
            file=sys.stderr,
        )
    if failing_p95:
        print(
            "agent-capability-gauntlet: p95 lane budget failures: "
            + ", ".join(failing_p95),
            file=sys.stderr,
        )
    if failing_regression:
        print(
            "agent-capability-gauntlet: regression budget failures: "
            + ", ".join(failing_regression),
            file=sys.stderr,
        )
    if failing_history_min:
        print(
            "agent-capability-gauntlet: insufficient per-workflow history for enforcement: "
            + ", ".join(failing_history_min),
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
