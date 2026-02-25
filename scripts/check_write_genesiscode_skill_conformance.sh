#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

MANIFEST_PATH="${GENESIS_WRITE_SKILL_CONFORMANCE_MANIFEST:-docs/skill_pack/write_genesiscode_v1/manifest.json}"
GAUNTLET_REPORT="${GENESIS_WRITE_SKILL_GAUNTLET_REPORT:-.genesis/perf/agent_capability_gauntlet_report.json}"
GENERATIVE_REPORT="${GENESIS_WRITE_SKILL_GENERATIVE_REPORT:-.genesis/perf/agent_generative_workloads_report.json}"
RUNTIME_BACKEND_REPORT="${GENESIS_WRITE_SKILL_RUNTIME_BACKEND_REPORT:-.genesis/perf/runtime_backend_feature_matrix_report.json}"
HOST_BRIDGE_REPORT="${GENESIS_WRITE_SKILL_HOST_BRIDGE_REPORT:-.genesis/perf/host_bridge_fault_injection_report.json}"
GPU_XR_REPORT="${GENESIS_WRITE_SKILL_GPU_XR_REPORT:-.genesis/perf/gpu_xr_productization_kits_report.json}"
ASSURANCE_REPORT="${GENESIS_WRITE_SKILL_ASSURANCE_REPORT:-.genesis/perf/assurance_profile_packs_report.json}"
REPORT_PATH="${GENESIS_WRITE_SKILL_CONFORMANCE_REPORT:-.genesis/perf/write_genesiscode_skill_conformance_report.json}"
HISTORY_PATH="${GENESIS_WRITE_SKILL_CONFORMANCE_HISTORY:-.genesis/perf/write_genesiscode_skill_conformance_history.jsonl}"
PROFILE="${GENESIS_WRITE_SKILL_CONFORMANCE_PROFILE:-${GENESIS_AGENT_GAUNTLET_PROFILE:-prepush-standard}}"
AUTO_RUN="${GENESIS_WRITE_SKILL_CONFORMANCE_AUTO_RUN:-1}"
MIN_SCORE="${GENESIS_WRITE_SKILL_CONFORMANCE_MIN_SCORE:-100}"
MIN_GENERATIVE_CASES="${GENESIS_WRITE_SKILL_CONFORMANCE_MIN_GENERATIVE_CASES:-8}"

[[ "$AUTO_RUN" == "0" || "$AUTO_RUN" == "1" ]] || {
  echo "write-genesiscode-skill-conformance: GENESIS_WRITE_SKILL_CONFORMANCE_AUTO_RUN must be 0 or 1" >&2
  exit 2
}

if [[ "$AUTO_RUN" == "1" ]]; then
  if [[ ! -f "$GAUNTLET_REPORT" ]]; then
    GENESIS_AGENT_GAUNTLET_PROFILE="$PROFILE" \
      bash scripts/check_agent_reference_workflows.sh
  fi
  if [[ ! -f "$GENERATIVE_REPORT" ]]; then
    GENESIS_AGENT_PARITY_GAUNTLET_PROFILE="$PROFILE" \
      bash scripts/check_agent_workflow_runtime_parity.sh
    GENESIS_AGENT_GENERATIVE_PRIMARY_REPORT=".genesis/perf/agent_capability_gauntlet_native_report.json" \
    GENESIS_AGENT_GENERATIVE_SECONDARY_REPORT=".genesis/perf/agent_capability_gauntlet_wasi_report.json" \
    GENESIS_AGENT_GENERATIVE_REQUIRE_SECONDARY=1 \
      bash scripts/check_agent_generative_workloads.sh
  fi
  if [[ ! -f "$RUNTIME_BACKEND_REPORT" ]]; then
    bash scripts/check_runtime_backend_feature_matrix.sh
  fi
  if [[ ! -f "$HOST_BRIDGE_REPORT" ]]; then
    bash scripts/check_host_bridge_fault_injection.sh
  fi
  if [[ ! -f "$GPU_XR_REPORT" ]]; then
    GENESIS_GPU_XR_PRODUCTIZATION_AUTO_RUN_GAUNTLET=1 \
    GENESIS_GPU_XR_REQUIRE_WEBXR_RUNTIME_EVIDENCE=1 \
      bash scripts/check_gpu_xr_productization_kits.sh
  fi
  if [[ ! -f "$ASSURANCE_REPORT" ]]; then
    bash scripts/check_assurance_profile_packs.sh
  fi
fi

required_files=(
  "$MANIFEST_PATH"
  "$GAUNTLET_REPORT"
  "$GENERATIVE_REPORT"
  "$RUNTIME_BACKEND_REPORT"
  "$HOST_BRIDGE_REPORT"
  "$GPU_XR_REPORT"
  "$ASSURANCE_REPORT"
)
for path in "${required_files[@]}"; do
  [[ -f "$path" ]] || {
    echo "write-genesiscode-skill-conformance: missing required input: $path" >&2
    exit 1
  }
done

python3 - \
  "$MANIFEST_PATH" \
  "$GAUNTLET_REPORT" \
  "$GENERATIVE_REPORT" \
  "$RUNTIME_BACKEND_REPORT" \
  "$HOST_BRIDGE_REPORT" \
  "$GPU_XR_REPORT" \
  "$ASSURANCE_REPORT" \
  "$REPORT_PATH" \
  "$HISTORY_PATH" \
  "$MIN_SCORE" \
  "$MIN_GENERATIVE_CASES" \
  "$PROFILE" <<'PY'
import datetime as dt
import json
import pathlib
import sys
from typing import Callable

(
    manifest_path_s,
    gauntlet_path_s,
    generative_path_s,
    runtime_backend_path_s,
    host_bridge_path_s,
    gpu_xr_path_s,
    assurance_path_s,
    report_path_s,
    history_path_s,
    min_score_s,
    min_generative_cases_s,
    profile,
) = sys.argv[1:]

manifest_path = pathlib.Path(manifest_path_s)
gauntlet_path = pathlib.Path(gauntlet_path_s)
generative_path = pathlib.Path(generative_path_s)
runtime_backend_path = pathlib.Path(runtime_backend_path_s)
host_bridge_path = pathlib.Path(host_bridge_path_s)
gpu_xr_path = pathlib.Path(gpu_xr_path_s)
assurance_path = pathlib.Path(assurance_path_s)
report_path = pathlib.Path(report_path_s)
history_path = pathlib.Path(history_path_s)
min_score = int(min_score_s)
min_generative_cases = int(min_generative_cases_s)

if min_score < 0 or min_score > 100:
    raise SystemExit("write-genesiscode-skill-conformance: min score must be in [0, 100]")
if min_generative_cases <= 0:
    raise SystemExit("write-genesiscode-skill-conformance: min generative cases must be > 0")

manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
if manifest.get("kind") != "genesis/write-genesiscode-skill-distribution-v1":
    raise SystemExit(
        f"write-genesiscode-skill-conformance: unexpected manifest kind: {manifest.get('kind')!r}"
    )
dist_req = manifest.get("distribution_requirements")
if not isinstance(dist_req, dict):
    raise SystemExit("write-genesiscode-skill-conformance: manifest missing distribution_requirements")
required_domains = dist_req.get("required_recipe_domains")
if not isinstance(required_domains, list) or not required_domains:
    raise SystemExit("write-genesiscode-skill-conformance: required_recipe_domains must be a non-empty list")
if not all(isinstance(x, str) and x.strip() for x in required_domains):
    raise SystemExit("write-genesiscode-skill-conformance: required_recipe_domains contains invalid entries")

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

runtime_backend = json.loads(runtime_backend_path.read_text(encoding="utf-8"))
if runtime_backend.get("kind") != "genesis/runtime-backend-feature-matrix-v0.1":
    raise SystemExit(
        "write-genesiscode-skill-conformance: unexpected runtime backend report kind: "
        f"{runtime_backend.get('kind')!r}"
    )

host_bridge = json.loads(host_bridge_path.read_text(encoding="utf-8"))
if host_bridge.get("kind") != "genesis/host-bridge-fault-injection-v0.1":
    raise SystemExit(
        f"write-genesiscode-skill-conformance: unexpected host bridge report kind: {host_bridge.get('kind')!r}"
    )

gpu_xr = json.loads(gpu_xr_path.read_text(encoding="utf-8"))
if gpu_xr.get("kind") != "genesis/gpu-xr-productization-kits-v0.1":
    raise SystemExit(
        f"write-genesiscode-skill-conformance: unexpected gpu/xr report kind: {gpu_xr.get('kind')!r}"
    )

assurance = json.loads(assurance_path.read_text(encoding="utf-8"))
if assurance.get("kind") != "genesis/assurance-profile-packs-v0.1":
    raise SystemExit(
        f"write-genesiscode-skill-conformance: unexpected assurance report kind: {assurance.get('kind')!r}"
    )

workflows = {}
for row in gauntlet.get("workflows", []):
    if not isinstance(row, dict):
        continue
    name = row.get("name")
    if isinstance(name, str):
        workflows[name] = row


def workflow_detail(
    *,
    rubric_id: str,
    workflow_name: str,
    required_domains: set[str],
) -> dict:
    row = workflows.get(workflow_name)
    if row is None:
        return {
            "rubric_id": rubric_id,
            "workflow": workflow_name,
            "ok": False,
            "error": "workflow-missing",
            "required_domains": sorted(required_domains),
            "observed_domains": [],
        }
    observed_domains = set(row.get("domains") or [])
    missing_domains = sorted(required_domains - observed_domains)
    ok = (
        bool(row.get("ok", False))
        and bool(row.get("exit_ok", False))
        and bool(row.get("replay_signal", False))
        and bool(row.get("duration_ok", False))
        and not missing_domains
    )
    return {
        "rubric_id": rubric_id,
        "workflow": workflow_name,
        "ok": ok,
        "required_domains": sorted(required_domains),
        "observed_domains": sorted(observed_domains),
        "missing_domains": missing_domains,
        "duration_ms": row.get("duration_ms"),
        "max_ms": row.get("max_ms"),
        "runtime_profile": gauntlet.get("runtime_profile"),
    }


def any_workflow_detail(
    *,
    rubric_id: str,
    workflow_names: list[str],
    required_domains: set[str],
) -> dict:
    details = [
        workflow_detail(
            rubric_id=rubric_id,
            workflow_name=workflow_name,
            required_domains=required_domains,
        )
        for workflow_name in workflow_names
    ]
    for detail in details:
        if detail.get("ok", False):
            detail["candidate_workflows"] = workflow_names
            return detail
    best = details[0]
    best["candidate_workflows"] = workflow_names
    best["all_candidates"] = details
    return best


def check_service() -> dict:
    return any_workflow_detail(
        rubric_id="service",
        workflow_names=["agent_service_workflow"],
        required_domains={"service", "package_publish_sync"},
    )


def check_graphics() -> dict:
    return any_workflow_detail(
        rubric_id="graphics",
        workflow_names=["agent_long_running_gfx_loop_workflow", "agent_interactive_gfx_compute_workflow"],
        required_domains={"graphics"},
    )


def check_gpu_compute() -> dict:
    return any_workflow_detail(
        rubric_id="gpu_compute",
        workflow_names=["agent_gpu_compute_workflow", "agent_compute_workflow"],
        required_domains={"gpu_compute"},
    )


def check_package_publish_sync() -> dict:
    return any_workflow_detail(
        rubric_id="package_publish_sync",
        workflow_names=["agent_multi_package_publish_workflow", "agent_service_workflow"],
        required_domains={"package_publish_sync"},
    )


def check_required_workflows(*, rubric_id: str, workflow_specs: list[tuple[str, set[str]]]) -> dict:
    checks = [
        workflow_detail(
            rubric_id=rubric_id,
            workflow_name=wf,
            required_domains=domains,
        )
        for wf, domains in workflow_specs
    ]
    ok = all(bool(item.get("ok", False)) for item in checks)
    return {
        "rubric_id": rubric_id,
        "ok": ok,
        "checks": checks,
        "required_workflows": [wf for wf, _ in workflow_specs],
    }


def check_process_lifecycle() -> dict:
    return any_workflow_detail(
        rubric_id="process_lifecycle",
        workflow_names=["agent_process_lifecycle_workflow"],
        required_domains={"process_lifecycle"},
    )


def check_filesystem() -> dict:
    return any_workflow_detail(
        rubric_id="filesystem",
        workflow_names=["agent_filesystem_workflow"],
        required_domains={"filesystem"},
    )


def check_network_process() -> dict:
    return any_workflow_detail(
        rubric_id="network_process",
        workflow_names=["agent_network_process_workflow"],
        required_domains={"network_process", "service"},
    )


def check_raw_network_sockets() -> dict:
    return any_workflow_detail(
        rubric_id="raw_network_sockets",
        workflow_names=["agent_raw_network_sockets_workflow"],
        required_domains={"raw_network_sockets"},
    )


def check_inbound_server() -> dict:
    return any_workflow_detail(
        rubric_id="inbound_server",
        workflow_names=["agent_inbound_server_workflow"],
        required_domains={"inbound_server"},
    )


def check_time_control() -> dict:
    return any_workflow_detail(
        rubric_id="time_control",
        workflow_names=["agent_time_control_workflow"],
        required_domains={"time_control"},
    )


def check_multi_agent_orchestration() -> dict:
    return any_workflow_detail(
        rubric_id="multi_agent_orchestration",
        workflow_names=["agent_multi_agent_orchestration_workflow"],
        required_domains={"multi_agent_orchestration"},
    )


def check_realtime_collaboration() -> dict:
    return any_workflow_detail(
        rubric_id="realtime_collaboration",
        workflow_names=["agent_realtime_collaboration_workflow"],
        required_domains={"realtime_collaboration"},
    )


def check_backend_topology() -> dict:
    return any_workflow_detail(
        rubric_id="backend_topology",
        workflow_names=["agent_backend_topology_workflow"],
        required_domains={"backend_topology"},
    )


def check_browser_runtime() -> dict:
    return any_workflow_detail(
        rubric_id="browser_runtime",
        workflow_names=["agent_browser_runtime_workflow"],
        required_domains={"browser_runtime"},
    )


def check_ml_data_engineering() -> dict:
    return check_required_workflows(
        rubric_id="ml_data_engineering",
        workflow_specs=[
            ("agent_ml_pipeline_variant_workflow", {"ml_pipeline_variant"}),
            ("agent_durable_data_workflow", {"durable_data"}),
        ],
    )


def check_complex_ui_app_stacks() -> dict:
    return check_required_workflows(
        rubric_id="complex_ui_app_stacks",
        workflow_specs=[
            ("agent_browser_runtime_workflow", {"browser_runtime"}),
            ("agent_interactive_gfx_compute_workflow", {"graphics", "gpu_compute"}),
            ("agent_xr_runtime_workflow", {"xr_runtime"}),
        ],
    )


def check_hardware_device_integration() -> dict:
    return check_required_workflows(
        rubric_id="hardware_device_integration",
        workflow_specs=[
            ("agent_gpu_compute_workflow", {"gpu_compute"}),
            ("agent_xr_runtime_workflow", {"xr_runtime"}),
            ("agent_plugin_runtime_workflow", {"plugin_runtime"}),
        ],
    )


def check_security_auth_services() -> dict:
    return check_required_workflows(
        rubric_id="security_auth_services",
        workflow_specs=[
            ("agent_backend_topology_workflow", {"backend_topology"}),
            ("agent_service_workflow", {"service", "package_publish_sync"}),
            ("agent_plugin_runtime_workflow", {"plugin_runtime"}),
        ],
    )


def check_deployment_targets() -> dict:
    workflow_specs = [
        ("agent_deploy_ios_workflow", {"deployment", "deploy_ios"}),
        ("agent_deploy_android_workflow", {"deployment", "deploy_android"}),
        ("agent_deploy_edge_workflow", {"deployment", "deploy_edge"}),
        ("agent_deploy_service_runtime_workflow", {"deployment", "deploy_service_runtime"}),
    ]
    return check_required_workflows(rubric_id="deployment_targets", workflow_specs=workflow_specs)


def check_failure_recovery() -> dict:
    families = host_bridge.get("families")
    if not isinstance(families, list):
        families = []
    required_families = {"fs", "net", "process", "plugin"}
    observed_families = {x for x in families if isinstance(x, str)}
    missing_families = sorted(required_families - observed_families)
    ok = (
        bool(host_bridge.get("ok", False))
        and bool(host_bridge.get("deterministic_replay_verified", False))
        and not missing_families
    )
    return {
        "rubric_id": "failure_recovery",
        "ok": ok,
        "report": str(host_bridge_path),
        "required_families": sorted(required_families),
        "observed_families": sorted(observed_families),
        "missing_families": missing_families,
        "runs": host_bridge.get("runs"),
        "failed_runs": host_bridge.get("failed_runs"),
        "observed_failure_rate_pct": host_bridge.get("observed_failure_rate_pct"),
    }


def check_performance_triage() -> dict:
    stage_count = runtime_backend.get("stage_count")
    if not isinstance(stage_count, int):
        stage_count = 0
    ok = bool(runtime_backend.get("ok", False)) and stage_count > 0
    return {
        "rubric_id": "performance_triage",
        "ok": ok,
        "report": str(runtime_backend_path),
        "stage_count": stage_count,
        "elapsed_ms": runtime_backend.get("elapsed_ms"),
        "budget_ms": runtime_backend.get("budget_ms"),
    }


def check_assurance() -> dict:
    profile_count = assurance.get("profile_count")
    if not isinstance(profile_count, int):
        profile_count = 0
    ok = bool(assurance.get("ok", False)) and profile_count >= 6
    return {
        "rubric_id": "assurance",
        "ok": ok,
        "report": str(assurance_path),
        "profile_count": profile_count,
    }


def check_plugin_ffi() -> dict:
    return any_workflow_detail(
        rubric_id="plugin_ffi",
        workflow_names=["agent_plugin_runtime_workflow"],
        required_domains={"plugin_runtime"},
    )


def check_xr_runtime() -> dict:
    return any_workflow_detail(
        rubric_id="xr_runtime",
        workflow_names=["agent_xr_runtime_workflow"],
        required_domains={"xr_runtime"},
    )


def check_xr_productization() -> dict:
    recipe_checks = gpu_xr.get("recipe_checks")
    if not isinstance(recipe_checks, dict):
        recipe_checks = {}
    workflow_checks = gpu_xr.get("workflow_checks")
    if not isinstance(workflow_checks, dict):
        workflow_checks = {}
    xr_recipe = recipe_checks.get("xr_deploy_test_workflow")
    xr_workflow = workflow_checks.get("agent_xr_runtime_workflow")
    ok = (
        bool(gpu_xr.get("ok", False))
        and isinstance(xr_recipe, dict)
        and bool(xr_recipe.get("ok", False))
        and isinstance(xr_workflow, dict)
        and bool(xr_workflow.get("ok", False))
        and bool(gpu_xr.get("webxr_runtime_evidence_present", False))
        and bool(gpu_xr.get("webxr_deterministic_replay", False))
    )
    return {
        "rubric_id": "xr_productization",
        "ok": ok,
        "report": str(gpu_xr_path),
        "recipe_ok": bool(isinstance(xr_recipe, dict) and xr_recipe.get("ok", False)),
        "workflow_ok": bool(isinstance(xr_workflow, dict) and xr_workflow.get("ok", False)),
        "webxr_runtime_evidence_present": bool(gpu_xr.get("webxr_runtime_evidence_present", False)),
        "webxr_deterministic_replay": bool(gpu_xr.get("webxr_deterministic_replay", False)),
    }


def check_durable_data() -> dict:
    return any_workflow_detail(
        rubric_id="durable_data",
        workflow_names=["agent_durable_data_workflow"],
        required_domains={"durable_data"},
    )


def check_gpu_non_graphics() -> dict:
    recipe_checks = gpu_xr.get("recipe_checks")
    if not isinstance(recipe_checks, dict):
        recipe_checks = {}
    workflow_checks = gpu_xr.get("workflow_checks")
    if not isinstance(workflow_checks, dict):
        workflow_checks = {}
    gpu_recipe = recipe_checks.get("gpu_data_simulation_workflow")
    compute_workflow = workflow_checks.get("agent_compute_workflow")
    gauntlet_compute = workflow_detail(
        rubric_id="gpu_non_graphics",
        workflow_name="agent_compute_workflow",
        required_domains={"gpu_compute"},
    )
    ok = (
        bool(gpu_xr.get("ok", False))
        and isinstance(gpu_recipe, dict)
        and bool(gpu_recipe.get("ok", False))
        and isinstance(compute_workflow, dict)
        and bool(compute_workflow.get("ok", False))
        and bool(gauntlet_compute.get("ok", False))
    )
    return {
        "rubric_id": "gpu_non_graphics",
        "ok": ok,
        "report": str(gpu_xr_path),
        "recipe_ok": bool(isinstance(gpu_recipe, dict) and gpu_recipe.get("ok", False)),
        "workflow_ok": bool(isinstance(compute_workflow, dict) and compute_workflow.get("ok", False)),
        "gauntlet_compute_ok": bool(gauntlet_compute.get("ok", False)),
        "gauntlet_compute_detail": gauntlet_compute,
    }


domain_handlers: dict[str, Callable[[], dict]] = {
    "service": check_service,
    "graphics": check_graphics,
    "gpu_compute": check_gpu_compute,
    "gpu_non_graphics": check_gpu_non_graphics,
    "package_publish_sync": check_package_publish_sync,
    "deployment_targets": check_deployment_targets,
    "failure_recovery": check_failure_recovery,
    "performance_triage": check_performance_triage,
    "assurance": check_assurance,
    "plugin_ffi": check_plugin_ffi,
    "xr_runtime": check_xr_runtime,
    "xr_productization": check_xr_productization,
    "durable_data": check_durable_data,
    "process_lifecycle": check_process_lifecycle,
    "filesystem": check_filesystem,
    "network_process": check_network_process,
    "raw_network_sockets": check_raw_network_sockets,
    "inbound_server": check_inbound_server,
    "time_control": check_time_control,
    "multi_agent_orchestration": check_multi_agent_orchestration,
    "realtime_collaboration": check_realtime_collaboration,
    "backend_topology": check_backend_topology,
    "browser_runtime": check_browser_runtime,
    "ml_data_engineering": check_ml_data_engineering,
    "complex_ui_app_stacks": check_complex_ui_app_stacks,
    "hardware_device_integration": check_hardware_device_integration,
    "security_auth_services": check_security_auth_services,
}

required_domains_ordered = []
seen_domains = set()
for domain in required_domains:
    if domain in seen_domains:
        continue
    seen_domains.add(domain)
    required_domains_ordered.append(domain)

domain_checks = []
for domain in required_domains_ordered:
    handler = domain_handlers.get(domain)
    if handler is None:
        domain_checks.append(
            {
                "rubric_id": domain,
                "domain": domain,
                "ok": False,
                "error": "unsupported-domain-handler",
            }
        )
        continue
    detail = handler()
    detail["domain"] = domain
    domain_checks.append(detail)

domain_count = len(domain_checks)
base_weight = 100 // domain_count if domain_count else 0
weight_remainder = 100 % domain_count if domain_count else 0
for index, detail in enumerate(domain_checks):
    weight = base_weight + (1 if index < weight_remainder else 0)
    detail["weight"] = weight
    detail["score"] = weight if bool(detail.get("ok", False)) else 0

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
generative_check = {
    "rubric_id": "generative_mutation_suite",
    "ok": generative_ok,
    "weight": 0,
    "score": 0,
    "required_case_count": min_generative_cases,
    "observed_case_count": generative_case_count,
    "parity_mismatches": generative_parity_mismatches,
    "history_min_failures": generative_history_min_failures,
}

score = sum(int(item["score"]) for item in domain_checks)
domain_all_ok = all(bool(item.get("ok", False)) for item in domain_checks)
all_ok = domain_all_ok and generative_ok
threshold_ok = score >= min_score
ok = all_ok and threshold_ok

rubric = [*domain_checks, generative_check]
report = {
    "kind": "genesis/write-genesiscode-skill-conformance-v0.1",
    "ok": ok,
    "profile": profile,
    "runtime_profile": gauntlet.get("runtime_profile"),
    "manifest_path": str(manifest_path),
    "required_domains": required_domains_ordered,
    "gauntlet_report": str(gauntlet_path),
    "generative_report": str(generative_path),
    "runtime_backend_report": str(runtime_backend_path),
    "host_bridge_report": str(host_bridge_path),
    "gpu_xr_report": str(gpu_xr_path),
    "assurance_report": str(assurance_path),
    "min_score": min_score,
    "score": score,
    "threshold_ok": threshold_ok,
    "domain_all_ok": domain_all_ok,
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
    "score": score,
    "min_score": min_score,
    "timestamp_utc": report["timestamp_utc"],
}
with history_path.open("a", encoding="utf-8") as fh:
    fh.write(json.dumps(history_entry, sort_keys=True) + "\n")

print(
    "write-genesiscode-skill-conformance: "
    f"report={report_path} ok={ok} score={score}/{min_score}"
)
if not ok:
    failures = [item["rubric_id"] for item in rubric if not bool(item.get("ok", False))]
    raise SystemExit(
        "write-genesiscode-skill-conformance: failing rubric categories: "
        + ", ".join(failures)
        + f"; score={score} min_score={min_score}"
    )
PY
