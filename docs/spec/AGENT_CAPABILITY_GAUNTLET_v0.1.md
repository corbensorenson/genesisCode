# Agent Capability Gauntlet v0.1

Deterministic multi-domain confidence gate for AI-first workflow readiness.

## Purpose

Validate that selfhost agent reference workflows pass across required product domains with replay-aware signals and bounded runtime budgets.

This gate is stricter than workflow smoke checks: it produces a scored report and fails closed on domain coverage regressions.

## Runner

- Script: `scripts/check_agent_reference_workflows.sh`
- Primary report: `.genesis/perf/agent_capability_gauntlet_report.json`
- History: `.genesis/perf/agent_capability_gauntlet_history.jsonl`
- Baseline seed history: `policies/perf/agent_capability_gauntlet_seed_history.jsonl`

## Report Contract

- `kind = "genesis/agent-capability-gauntlet-v0.1"`
- `ok` boolean
- `workflow_count`, `workflow_successes`, `score_percent`
- `domain_count`, `domain_successes`
- `required_domains` (sorted domain id list)
- `required_domain_thresholds` (domain -> minimum success count)
- `elapsed_ms`, `budget_ms`
- `history_samples`, `history_p95_ms`, `history_p95_enforced`, `history_p95_ok`
- `fail_reasons` (string list)
- `default_max_ms` (legacy compatibility alias for budget defaults)
- `p95_default_max_ms`, `p95_min_samples`, `p95_failures`
- `baseline_history_path`, `require_min_history`
- `regression_percent`, `regression_failures`, `history_min_failures`
- `profile`, `runtime_profile`, `genesis_bin`
- `require_gpu_device_backend`
- `confidence_lane` (`release-confidence-device` or `dev-fallback-evidence`)
- `release_confidence_lane`, `fallback_evidence_lane`
- `domains`:
  - `domain`
  - `required_successes`
  - `successes`
  - `ok`
- `workflows`:
  - `name`, `path`, `domains`
  - `exit_code`, `exit_ok`
  - `replay_signal`, `replay_value`, `replay_hash`
  - `replay_value_normalized`, `replay_hash_normalized`
  - `duration_ms`, `max_ms`, `duration_ok`
  - `p95_duration_ms`, `p95_budget_ms`, `p95_sample_count`, `p95_enforced`, `p95_ok`
  - `history_min_ok`, `require_min_history`, `baseline_history_sample_count`
  - `baseline_p95_ms`, `regression_percent`, `regression_enforced`, `regression_budget_ms`, `regression_ok`
  - `ok`

## Required Domains

The gate requires at least one successful workflow for each:

- `service`
- `network_process`
- `package_publish_sync`
- `graphics`
- `gpu_compute`
- `filesystem`
- `raw_network_sockets`
- `inbound_server`
- `durable_data`
- `process_lifecycle`
- `plugin_runtime`
- `time_control`
- `browser_runtime`
- `ui_application_stack`
- `xr_runtime`
- `auth_security_service`
- `hardware_device_integration`
- `data_pipeline_orchestration`
- `deployment`
- `deploy_ios`
- `deploy_android`
- `deploy_edge`
- `deploy_service_runtime`
- `multi_agent_orchestration`
- `realtime_collaboration`
- `ml_pipeline_variant`
- `backend_topology`

If any required domain misses its minimum success threshold, the script exits non-zero.

## Budget Controls

- `GENESIS_AGENT_GAUNTLET_DEFAULT_MAX_MS` (default `300000`)
- `GENESIS_AGENT_GAUNTLET_MAX_MS_<WORKFLOW_NAME_UPPER>` per-workflow override
- `GENESIS_AGENT_GAUNTLET_P95_DEFAULT_MAX_MS` (defaults to `GENESIS_AGENT_GAUNTLET_DEFAULT_MAX_MS`)
- `GENESIS_AGENT_GAUNTLET_P95_MAX_MS_<WORKFLOW_NAME_UPPER>` per-workflow p95 override
- `GENESIS_AGENT_GAUNTLET_P95_MIN_SAMPLES` minimum history samples before p95 enforcement (default `8`)
- `GENESIS_AGENT_GAUNTLET_BASELINE_HISTORY` default `policies/perf/agent_capability_gauntlet_seed_history.jsonl`
- `GENESIS_AGENT_GAUNTLET_REQUIRE_MIN_HISTORY` fail-closed on insufficient per-workflow history (default `1`)
- `GENESIS_AGENT_GAUNTLET_REGRESSION_PERCENT` per-workflow regression budget over baseline p95 (default `25`)

## Confidence Lanes

- `release-confidence-device`: `require_gpu_device_backend=true`; GPU workflows must prove `device-runtime`.
- `dev-fallback-evidence`: fallback-enabled lane for local development evidence only.

## CI Expectations

- `standard` and `full` CI profiles run this gate.
- Failures in workflow checks or domain thresholds fail CI.
- `scripts/check_slo_report_contracts.sh` enforces required SLO fields and
  fail-closed elapsed/p95 budget semantics on the emitted report.
- Release/full profile also runs `scripts/check_agent_workflow_runtime_parity.sh` to enforce native vs WASI/wasm-host bridge parity hash equivalence for the same workflows.
- `scripts/check_agent_scenario_perf.sh` consumes this gate's workflow durations to enforce aggregated multi-domain median+p95 scenario latency SLOs.
