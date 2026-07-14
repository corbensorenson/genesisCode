# Selfhost Readiness Scorecard v0.1

## Purpose

Evaluate whether
GenesisCode's strict selfhost command-routing profile, parity isolation, bootstrap-mode
configuration, selected runtime quality checks, and active P0/P1 defect closure satisfy
this scorecard, and define the deterministic machine-readable report produced on explicit
request. This report does not establish H2 semantic authority, an H3 cross-host
bootstrap fixpoint, L5 release evidence, or v1 release readiness.

## Runner

- Read-only check: `scripts/check_selfhost_readiness_scorecard.sh`
- Explicit producer: `scripts/update_selfhost_readiness_scorecard_report.sh`
- Optional primary report: `.genesis/perf/selfhost_readiness_report.json`
- Optional history: `.genesis/perf/selfhost_readiness_history.jsonl`

Strict profiles invoke dashboard freshness and this readiness scorecard as
separate checks. Neither check regenerates the other's local E0 report.
The readiness check renders into private temporary outputs while evaluating p95
against the configured existing history input. Only the explicit producer appends
to the retained history and replaces the retained report.

Default history floor:

- `GENESIS_SELFHOST_READINESS_P95_MIN_SAMPLES=5`

## Report Contract

- `kind = "genesis/selfhost-readiness-v0.1"`
- `ok` boolean
- `score_percent`
- `elapsed_ms`, `budget_ms`, `history_samples`, `history_p95_ms`, `history_p95_enforced`, `history_p95_ok`
- `p95_min_samples`
- `fail_reasons` (string list)
  - canonical closure token: `unresolved-upgrade-plan-ids`
  - backward-compatible legacy token accepted by cutover gates: `open-upgrade-plan-ids`
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

`critical_gate_truth` is a fail-closed aggregate over machine reports:

- `.genesis/perf/agent_capability_gauntlet_release_confidence_report.json`
- `.genesis/perf/agent_generative_workloads_report.json`
- `.genesis/perf/agent_workflow_runtime_parity_report.json`
- `.genesis/perf/production_cli_help_surface_report.json`
- `.genesis/perf/gpu_gfx_headroom_conformance_report.json`
- `.genesis/perf/domain_starter_registry_bootstrap_report.json`
- `.genesis/perf/gcpm_target_runtime_evidence_report.json`

Produce domain-starter registry evidence explicitly with
`scripts/update_domain_starter_registry_bootstrap_report.sh`; the read-only
scorecard never refreshes this prerequisite.

Target runtime evidence report contract:

- producer: `scripts/update_gcpm_target_runtime_pipelines_report.sh`
- `kind = "genesis/gcpm-target-runtime-evidence-v0.1"`
- per-target runtime evidence payload includes:
  - runtime mode (`synthetic-adapter` or `non-synthetic`)
  - runtime class (`emulator|device|container|host-runtime|synthetic-adapter`)
  - replay artifact directory + stdout/stderr hashes
- strict policy:
  - `GENESIS_GCPM_TARGET_RUNTIME_REQUIRE_NON_SYNTHETIC=1` requires non-synthetic evidence per target and fails closed
  - default strictness follows CI context (`CI=true` => strict)

GPU/GFX headroom conformance must include lane backend metadata consumed by readiness:

- `require_device_lane_mode`, `require_device_lane_active`, `device_runtime_available`
- `lanes.normal.backend_policy = "require-device"` and `lanes.normal.expected_backend = "device-runtime"` whenever device runtime is available
- `lanes.low-headroom.fallback_policy = "allow-fallback-under-headroom"` with observed backend evidence

The scorecard is read-only with respect to prerequisite reports. Produce the gauntlet input
separately in strict release-confidence mode before running the scorecard:

- `GENESIS_AGENT_GAUNTLET_PROFILE=release-full`
- `GENESIS_AGENT_GAUNTLET_REQUIRE_GPU_DEVICE_BACKEND=1`
- `require_gpu_device_backend=true`
- `confidence_lane="release-confidence-device"`

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

`ok=true` closes only this scorecard contract. Semantic selfhost authority is reported in
`docs/status/SELFHOST_AUTHORITY_v0.1.md`; release-claim eligibility is reported in
`feature_matrix.md`. Neither may be inferred from this mutable local report.

## Runtime History Floor Inputs

Runtime/perf reports consumed by readiness are expected to be produced with
their own seeded baseline-history + minimum-sample contract. For migrated
report sets, use the explicit producer:

- `scripts/update_runtime_microbench_budgets_report.sh`
- `scripts/update_hot_path_budgets_report.sh`
- `scripts/update_perf_budgets_report.sh`
- `scripts/update_task_concurrency_stress_report.sh`
- `scripts/check_production_cli_help_surface.sh`
- `scripts/update_gpu_compute_runtime_profile_report.sh`
- `scripts/check_gfx_runtime_profile.sh`

Default baseline seed files live under `policies/perf/*_seed_history.jsonl`.

Default mode is non-strict evaluation (always renders an ephemeral report). Set
`GENESIS_SELFHOST_READINESS_STRICT=1` to make non-ready status fail the gate.
