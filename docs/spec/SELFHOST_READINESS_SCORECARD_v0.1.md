# Selfhost Readiness Scorecard v0.1

## Purpose

Emit a deterministic machine-readable readiness report that tracks whether
GenesisCode is ready for a strict selfhost v1 cutover.

## Runner

- Script: `scripts/check_selfhost_readiness_scorecard.sh`
- Primary report: `.genesis/perf/selfhost_readiness_report.json`
- History: `.genesis/perf/selfhost_readiness_history.jsonl`

The runner is also invoked by `scripts/check_selfhost_dashboard_fresh.sh` so
dashboard freshness and readiness scoring stay coupled.

Default history floor:

- `GENESIS_SELFHOST_READINESS_P95_MIN_SAMPLES=5`

## Report Contract

- `kind = "genesis/selfhost-readiness-v0.1"`
- `ok` boolean
- `score_percent`
- `elapsed_ms`, `budget_ms`, `history_samples`, `history_p95_ms`, `history_p95_enforced`, `history_p95_ok`
- `p95_min_samples`
- `fail_reasons` (string list)
- `unresolved_upgrade_plan_ids` (string list), `closure_ok`
- `dimensions` object with scored dimensions:
  - `runtime_routing_coverage`
  - `parity_only_surface_isolation`
  - `bootstrap_mode_strictness`
  - `deprecated_bootstrap_reference_count`
  - `critical_gate_truth`
  - `runtime_quality_truth`

`runtime_quality_truth` is a fail-closed aggregate over machine reports:

- `.genesis/perf/runtime_microbench_runtime_report.json`
- `.genesis/perf/hot_path_runtime_report.json`
- `.genesis/perf/task_concurrency_stress_report.json`
- `.genesis/perf/host_api_evolution_contract_report.json`

Each dimension records at least:

- `ok`
- `score`
- `max_score`
- dimension-specific evidence fields

## Closure Semantics

Readiness is `ok=true` only when all are true:

1. All scored dimensions are `ok=true`.
2. No unresolved `upgrade_plan.md` checklist IDs remain.
3. Runtime elapsed and history-p95 remain within configured budget.

## Runtime History Floor Inputs

Runtime/perf reports consumed by readiness are expected to enforce their own
seeded baseline-history + minimum-sample contract:

- `scripts/check_runtime_microbench_budgets.sh`
- `scripts/check_hot_path_budgets.sh`
- `scripts/check_perf_budgets.sh`
- `scripts/check_production_cli_help_surface.sh`
- `scripts/check_gpu_compute_runtime_profile.sh`
- `scripts/check_gfx_runtime_profile.sh`

Default baseline seed files live under `policies/perf/*_seed_history.jsonl`.

Default mode is non-strict reporting (always emits report). Set
`GENESIS_SELFHOST_READINESS_STRICT=1` to make non-ready status fail the gate.
