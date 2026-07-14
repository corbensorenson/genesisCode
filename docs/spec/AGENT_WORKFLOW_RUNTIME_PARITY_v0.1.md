# Agent Workflow Runtime Parity v0.1

## Purpose

Enforce that agent reference workflows stay replay-deterministic across:

- native selfhost CLI runtime (`genesis`)
- WASI/wasm-host bridge runtime posture (`genesis_wasi`)

This is a strict parity gate, not a smoke test.

## Runner

- Read-only check: `scripts/check_agent_workflow_runtime_parity.sh`
- Explicit producer: `scripts/update_agent_workflow_runtime_parity_report.sh`
- Optional primary report: `.genesis/perf/agent_workflow_runtime_parity_report.json`
- Optional history: `.genesis/perf/agent_workflow_runtime_parity_history.jsonl`
- Optional mutation parity companion report: `.genesis/perf/agent_generative_workloads_parity_report.json`
- Default minimum history floor for p95 enforcement: `GENESIS_AGENT_PARITY_P95_MIN_SAMPLES=8`

## Inputs

For a fresh run, the parity renderer executes the reference-workflow renderer twice in parallel:

1. native lane (`runtime_profile = "native"`)
2. wasi lane (`runtime_profile = "wasi-wasm-host-bridge"`)

Reference workflow set includes browser-runtime, XR runtime, and deployment lanes, so
parity checks cover all platform runtime additions automatically via shared gauntlet inputs.

After native/wasi gauntlets complete, the parity renderer invokes the generative-workload
renderer with both lane reports to verify replay-digest
parity across deterministic mutation cases derived from the shared workflow pool.

Validated retained native/WASI reports may be reused when their kind, profile,
runtime profile, status, and freshness satisfy the reuse contract. Checks never
refresh those retained reports; only the explicit producer retains fresh lane,
generative, aggregate, and history outputs.

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
- `history_samples`, `history_p95_ms`, `history_p95_enforced`, `history_p95_ok`
- `p95_min_samples`
- `fail_reasons` (string list)
- `missing_native_workflows`, `missing_wasi_workflows`
- `workflow_mismatches` (includes raw replay hash, parity hash, and lane ok states)
- `domain_mismatches`

The SLO schema contract is validated by `scripts/check_slo_report_contracts.sh`.
