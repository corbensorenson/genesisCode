> Bundle Entry: `docs/spec/TESTING_BUNDLE_v0.1.md`
> Legacy Split Doc: Prefer the bundle entrypoint for agent retrieval; this file retains detailed, topic-local semantics.

# Agent End-to-End Scenario Perf v0.1

Purpose:
- Enforce a user-facing multi-domain scenario latency gate from existing gauntlet runs.
- Provide contention-aware median+p95 SLO checks without rerunning extra heavy suites.
- Fail release-profile gates when scenario latency regresses beyond configured thresholds.

## Runner

- Script: `scripts/check_agent_scenario_perf.sh`
- Report: `.genesis/perf/agent_scenario_perf_report.json`
- History: `.genesis/perf/agent_scenario_perf_history.jsonl`
- Baseline seed history: `policies/perf/agent_scenario_perf_seed_history.jsonl`

## Scenario Definition (Default)

The default scenario aggregates durations from these gauntlet workflows:
- `agent_service_workflow`
- `agent_durable_data_workflow`
- `agent_long_running_gfx_loop_workflow`
- `agent_network_process_workflow`

Each sample is the sum of the four workflow durations from one gauntlet run.

## Report Contract

- `kind = "genesis/agent-scenario-perf-v0.1"`
- `scenario_name`, `runtime_profile`, `required_workflows`
- `component_durations_ms`, `scenario_duration_ms`
- `samples_ms`, `sample_count`, `historical_sample_count`, `baseline_seed_sample_count`
- `median_ms`, `median_budget_ms`, `median_ok`
- `require_min_history`, `history_min_ok`
- `p95_ms`, `p95_budget_ms`, `p95_min_samples`, `p95_enforced`, `p95_ok`
- `baseline_p95_ms`, `regression_percent`, `regression_budget_ms`, `regression_ok`
- `contention_spread_percent`, `contention_warn_percent`, `contention_warning`
- `ok`

## Policy / Budget Controls

- `GENESIS_AGENT_SCENARIO_WORKFLOWS` CSV override for required workflow set.
- `GENESIS_AGENT_SCENARIO_MEDIAN_BUDGET_MS` default `600000`.
- `GENESIS_AGENT_SCENARIO_P95_BUDGET_MS` default `750000`.
- `GENESIS_AGENT_SCENARIO_P95_MIN_SAMPLES` default `8`.
- `GENESIS_AGENT_SCENARIO_BASELINE_HISTORY` default `policies/perf/agent_scenario_perf_seed_history.jsonl`.
- `GENESIS_AGENT_SCENARIO_REQUIRE_MIN_HISTORY` default `1` (fail-closed when sample depth is below `GENESIS_AGENT_SCENARIO_P95_MIN_SAMPLES`).
- `GENESIS_AGENT_SCENARIO_REGRESSION_PERCENT` default `25`.
- `GENESIS_AGENT_SCENARIO_CONTENTION_WARN_PERCENT` default `50`.

## Release Gate Integration

- `scripts/check_upgrade_plan_health.sh --profile release-full` runs this gate after the gauntlet.
- CI `standard|full` lanes run this gate immediately after `check_agent_reference_workflows.sh`.
