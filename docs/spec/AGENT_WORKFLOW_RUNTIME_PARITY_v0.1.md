# Agent Workflow Runtime Parity v0.1

## Purpose

Enforce that agent reference workflows stay replay-deterministic across:

- native selfhost CLI runtime (`genesis`)
- WASI/wasm-host bridge runtime posture (`genesis_wasi`)

This is a strict parity gate, not a smoke test.

## Runner

- Script: `scripts/check_agent_workflow_runtime_parity.sh`
- Primary report: `.genesis/perf/agent_workflow_runtime_parity_report.json`
- History: `.genesis/perf/agent_workflow_runtime_parity_history.jsonl`

## Inputs

The parity runner executes `scripts/check_agent_reference_workflows.sh` twice:

1. native lane (`runtime_profile = "native"`)
2. wasi lane (`runtime_profile = "wasi-wasm-host-bridge"`)

Each lane emits a gauntlet report with per-workflow:

- `replay_hash` (raw workflow replay payload hash)
- `replay_hash_normalized` (parity signal hash when domain-level normalization is required)

## Pass Criteria

- Every workflow exists in both lanes.
- Every workflow passes in both lanes.
- Every workflow parity hash matches between lanes
  - parity hash = `replay_hash_normalized` when present, else `replay_hash`
- Domain success counts/ok-status match between lanes.
- Total parity lane elapsed time stays within budget.

## Report Contract

- `kind = "genesis/agent-workflow-runtime-parity-v0.1"`
- `ok` boolean
- `gauntlet_profile`
- `native_bin`, `wasi_bin`
- `native_report`, `wasi_report`
- `workflow_count`, `domain_count`
- `elapsed_ms`, `budget_ms`
- `missing_native_workflows`, `missing_wasi_workflows`
- `workflow_mismatches` (includes raw replay hash, parity hash, and lane ok states)
- `domain_mismatches`
