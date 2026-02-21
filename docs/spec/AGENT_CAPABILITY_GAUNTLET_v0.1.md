# Agent Capability Gauntlet v0.1

Deterministic multi-domain confidence gate for AI-first workflow readiness.

## Purpose

Validate that selfhost agent reference workflows pass across required product domains with replay-aware signals and bounded runtime budgets.

This gate is stricter than workflow smoke checks: it produces a scored report and fails closed on domain coverage regressions.

## Runner

- Script: `scripts/check_agent_reference_workflows.sh`
- Primary report: `.genesis/perf/agent_capability_gauntlet_report.json`
- History: `.genesis/perf/agent_capability_gauntlet_history.jsonl`

## Report Contract

- `kind = "genesis/agent-capability-gauntlet-v0.1"`
- `ok` boolean
- `workflow_count`, `workflow_successes`, `score_percent`
- `domain_count`, `domain_successes`
- `elapsed_ms`, `default_max_ms`
- `domains`:
  - `domain`
  - `required_successes`
  - `successes`
  - `ok`
- `workflows`:
  - `name`, `path`, `domains`
  - `exit_code`, `exit_ok`
  - `replay_signal`
  - `duration_ms`, `max_ms`, `duration_ok`
  - `ok`

## Required Domains

The gate requires at least one successful workflow for each:

- `service`
- `network_process`
- `package_publish_sync`
- `graphics`
- `gpu_compute`

If any required domain misses its minimum success threshold, the script exits non-zero.

## Budget Controls

- `GENESIS_AGENT_GAUNTLET_DEFAULT_MAX_MS` (default `300000`)
- `GENESIS_AGENT_GAUNTLET_MAX_MS_<WORKFLOW_NAME_UPPER>` per-workflow override

## CI Expectations

- `standard` and `full` CI profiles run this gate.
- Failures in workflow checks or domain thresholds fail CI.
