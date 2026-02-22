> Bundle Entry: `docs/spec/TESTING_BUNDLE_v0.1.md`

# Agent Generative Workloads v0.1

Purpose:
- Extend agent validation beyond the fixed workflow catalog by generating deterministic
  synthetic workload compositions from successful gauntlet workflows.
- Feed both performance and parity lanes with mutation-based checks.

## Runner

- Script: `scripts/check_agent_generative_workloads.sh`
- Report: `.genesis/perf/agent_generative_workloads_report.json`
- History: `.genesis/perf/agent_generative_workloads_history.jsonl`
- Baseline seed history: `policies/perf/agent_generative_workloads_seed_history.jsonl`

## Inputs

- Primary gauntlet report (`GENESIS_AGENT_GENERATIVE_PRIMARY_REPORT`):
  - default `.genesis/perf/agent_capability_gauntlet_report.json`
- Optional secondary gauntlet report (`GENESIS_AGENT_GENERATIVE_SECONDARY_REPORT`):
  - when set, generated workload replay digests must match across both runtime reports.

Only successful workflows with deterministic replay hashes are included in the mutation pool.

## Mutation Model

For each generated case:
- choose a deterministic workflow subset (`min..max` bounds),
- mutate ordering/shape deterministically from seed and case index,
- compute:
  - aggregated duration (`sum(duration_ms)`),
  - domain coverage,
  - replay digest (`sha256` over ordered replay-hash components).

## Pass Criteria

- each generated case satisfies minimum domain coverage,
- each generated case satisfies duration budget,
- optional history-aware regression gates pass (when enough history exists),
- fail-closed minimum-history policy is enforced by default (`require_min_history=1`),
- when secondary report is provided, case replay digests match across both reports.

## Configuration

- `GENESIS_AGENT_GENERATIVE_CASE_COUNT` (default `12`)
- `GENESIS_AGENT_GENERATIVE_MIN_WORKFLOWS` (default `3`)
- `GENESIS_AGENT_GENERATIVE_MAX_WORKFLOWS` (default `6`)
- `GENESIS_AGENT_GENERATIVE_MIN_DOMAIN_COUNT` (default `2`)
- `GENESIS_AGENT_GENERATIVE_MAX_CASE_DURATION_MS` (default `600000`)
- `GENESIS_AGENT_GENERATIVE_P95_MIN_SAMPLES` (default `8`)
- `GENESIS_AGENT_GENERATIVE_REGRESSION_PERCENT` (default `25`)
- `GENESIS_AGENT_GENERATIVE_BASELINE_HISTORY` (default `policies/perf/agent_generative_workloads_seed_history.jsonl`)
- `GENESIS_AGENT_GENERATIVE_REQUIRE_MIN_HISTORY` (default `1`)
- `GENESIS_AGENT_GENERATIVE_SEED` (default `genesis-agent-generative-v1`)

## Report Contract

- `kind = "genesis/agent-generative-workloads-v0.1"`
- `ok`, `seed`, `runtime_profile`, optional `secondary_runtime_profile`
- case generation bounds and budgets
- summary duration stats
- `baseline_history_path`, `require_min_history`
- `duration_failures`, `domain_failures`, `regression_failures`, `history_min_failures`, `parity_mismatches`
- per-case records (workflow set, domain set, duration, replay digest, parity/regression fields)
