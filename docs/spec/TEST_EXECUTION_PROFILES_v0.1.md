> Bundle Entry: `docs/spec/TESTING_BUNDLE_v0.1.md`
> Legacy Split Doc: Prefer the bundle entrypoint for agent retrieval; this file retains detailed, topic-local semantics.

# Test Execution Profiles v0.1

Deterministic test execution policy for local iteration and CI.

## Goals

- Keep local high-signal feedback loops fast (`<=5m` default target).
- Preserve full suite coverage in CI profiles.
- Keep shard assignment deterministic across runs.

## Tiered Matrix (Normative Budgets)

| Profile | Primary command(s) | Runtime budget |
|---|---|---|
| `smoke` | `bash scripts/selfhost_strict_smoke.sh` | `<= 3m` |
| `changed-fast` | `bash scripts/test_changed_fast.sh --budget-ms 300000` | `<= 5m` |
| `strict-golden` | `bash scripts/selfhost_strict_golden.sh` | `<= 15m` |
| `full-cross-host` | strict golden + `node scripts/wasm_cross_host_determinism.mjs` + `bash scripts/check_full_cross_host_profile_budget.sh` | `<= 20m` |

The local default high-signal workflow is `changed-fast` with a hard 300000ms budget.

Strict/full profile runtime reports:
- `strict-golden`
  - report: `.genesis/perf/strict_golden_profile_report.json`
  - history: `.genesis/perf/strict_golden_profile_history.jsonl`
  - enforced by `scripts/selfhost_strict_golden.sh` via measured elapsed + history p95 checks.
- `wasm-cross-host`
  - report: `.genesis/perf/wasm_cross_host_profile_report.json`
  - history: `.genesis/perf/wasm_cross_host_profile_history.jsonl`
  - enforced by `scripts/wasm_cross_host_determinism.mjs` via measured elapsed + history p95 checks.
- `full-cross-host` aggregate lane
  - report: `.genesis/perf/full_cross_host_profile_report.json`
  - history: `.genesis/perf/full_cross_host_profile_history.jsonl`
  - enforced by `scripts/check_full_cross_host_profile_budget.sh` as strict-golden + wasm-cross-host elapsed sum with history p95 gate.

## Runners

- Preferred runner: `cargo nextest` (configured by `/.config/nextest.toml`).
- Fallback runner: `cargo test` when nextest is unavailable.

## Local

- Default fast loop: `scripts/test_changed_fast.sh`
  - changed-file aware selection (or clean-tree fallback)
  - warms selfhost artifact cache when relevant paths change
  - emits deterministic metrics report `kind = genesis/test-changed-fast-metrics-v0.1`
  - default hard budget: 300000ms (`GENESIS_TEST_CHANGED_BUDGET_MS`)
- Alias wrapper: `scripts/test_fast.sh`
  - defaults to `scripts/test_changed_fast.sh`
  - pass `--full` to run the broad fast suite
- Full fast fallback: `scripts/test_fast_full.sh`
  - auto-detects nextest
  - runs high-signal core libs + selected CLI integration tests
- Full/sharded loop: `scripts/test_shard_workspace.sh --total N --index I --runner auto|nextest|cargo`
  - deterministic shard assignment by `(seed, crate)` hash
  - emits report `kind = genesis/test-shard-report-v0.1`
- Prepush strict loop: `scripts/check_upgrade_plan_health.sh --profile prepush-standard`
  - defaults to deterministic gate sharding (`GENESIS_HEALTH_SHARDS`) derived from host parallelism
    for non-release loops (2-way on small hosts, 4-way on larger hosts)
  - defaults profile gates to serial execution (`GENESIS_HEALTH_PROFILE_SHARDS=1`) to reduce
    cargo build-lock contention while preserving full gate coverage
  - emits profile report `kind = genesis/upgrade-plan-health-profile-v0.1` at
    `.genesis/perf/upgrade_plan_health_profile_report.json`
  - enforces prepush wall-time budget `GENESIS_HEALTH_PREPUSH_BUDGET_MS` (default `240000`)
    whenever health gates are enforced
  - GPU device-conformance lane policy:
    - `release-full` profile requires `scripts/check_gpu_compute_device_conformance.sh` by default.
    - `dev-fast` and `prepush-standard` remain opt-in via
      `GENESIS_HEALTH_REQUIRE_GPU_DEVICE_CONFORMANCE=1`.

## CI Profiles

- `fast`: runs `scripts/test_changed_fast.sh` (default local/CI fast path)
- `standard|full`:
  - installs nextest
  - uses deterministic shard execution when `GENESIS_TEST_SHARDS_TOTAL > 1`
  - otherwise runs full workspace tests with nextest (`--cargo-profile selfhost-strict`)
  - preserves existing strict/smoke/golden gates as separate steps
  - runs `scripts/check_ai_stress_suite.sh` to enforce deterministic high-throughput stress
    coverage for tasks + bridge + gpu/compute + replay integrity.
- Iteration conformance check:
  - `scripts/check_default_iteration_workflow.sh` validates measurable fast-path execution and
    deterministic shard selection.
  - default budget for changed-fast in this check is 300000ms (`GENESIS_BUDGET_CHANGED_FAST_MS`).

## Drift Guard

Profile/budget drift is blocked by:
- `scripts/check_test_execution_profile_matrix.sh`

This guard enforces:
- matrix rows for `smoke`, `changed-fast`, `strict-golden`, `full-cross-host`
- explicit budget strings in this spec
- CI step presence for each matrix lane
- 300000ms default budget pin for local high-signal workflows
- prepush strict loop budget/shard defaults (`GENESIS_HEALTH_PREPUSH_BUDGET_MS`,
  `GENESIS_HEALTH_SHARDS`) and profile report kind
- strict/full measured runtime gate wiring:
  - strict-golden profile runtime report + p95 budget helper
  - wasm cross-host runtime report + p95 budget helper
  - full-cross-host aggregate runtime budget gate command in CI

## Determinism

- Shard selection is deterministic from:
  - shard total/index
  - seed (`GENESIS_TEST_SHARD_SEED` or `GITHUB_SHA` in CI)
  - stable sorted crate list
- Runner selection is explicit in reports (`runner = cargo|nextest`).
