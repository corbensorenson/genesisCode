> Bundle Entry: `docs/spec/TESTING_BUNDLE_v0.1.md`

# Test Execution Profiles v0.1

Deterministic test execution policy for local iteration and CI.

## Goals

- Keep local high-signal feedback loops fast (`<=2m` default target).
- Preserve full suite coverage in CI profiles.
- Keep shard assignment deterministic across runs.

## Tiered Matrix (Normative Budgets)

| Profile | Primary command(s) | Runtime budget |
|---|---|---|
| `smoke` | `bash scripts/selfhost_strict_smoke.sh` | `<= 3m` |
| `changed-fast` | `bash scripts/test_changed_fast.sh --budget-ms 120000` | `<= 2m` |
| `agent-inner-loop` | `bash scripts/check_upgrade_plan_health.sh --profile agent-inner-loop` | `<= 5m` |
| `strict-golden` | `bash scripts/selfhost_strict_golden.sh` | `<= 8m` |
| `full-cross-host` | strict golden + `node scripts/wasm_cross_host_determinism.mjs` + `bash scripts/check_full_cross_host_profile_budget.sh` | `<= 12m` |

The local default high-signal workflow is `changed-fast` with a hard 120000ms budget.

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
  - baseline seed history: `policies/perf/full_cross_host_profile_seed_history.jsonl`
  - enforced by `scripts/check_full_cross_host_profile_budget.sh` as strict-golden + wasm-cross-host elapsed sum with history p95 gate, plus fail-closed minimum-history enforcement.
- `agent-scenario-perf` aggregate lane
  - report: `.genesis/perf/agent_scenario_perf_report.json`
  - history: `.genesis/perf/agent_scenario_perf_history.jsonl`
  - baseline seed history: `policies/perf/agent_scenario_perf_seed_history.jsonl`
  - enforced by `scripts/check_agent_scenario_perf.sh` from gauntlet component durations (service + durable-data + gfx-loop + network-process) with median + p95 + regression gates and fail-closed minimum-history enforcement.
- `agent-generative-workloads` mutation lane
  - report: `.genesis/perf/agent_generative_workloads_report.json`
  - history: `.genesis/perf/agent_generative_workloads_history.jsonl`
  - baseline seed history: `policies/perf/agent_generative_workloads_seed_history.jsonl`
  - enforced by `scripts/check_agent_generative_workloads.sh` using deterministic mutation sets derived from successful gauntlet workflows, with fail-closed minimum-history + p95/regression enforcement and optional parity enforcement.
- `agent-capability-gauntlet` per-workflow performance lane
  - report: `.genesis/perf/agent_capability_gauntlet_report.json`
  - history: `.genesis/perf/agent_capability_gauntlet_history.jsonl`
  - baseline seed history: `policies/perf/agent_capability_gauntlet_seed_history.jsonl`
  - enforced by `scripts/check_agent_reference_workflows.sh` with per-workflow fail-closed minimum-history + p95/regression budgets (native and parity wasi lanes).
- `agent-inner-loop` health lane
  - report: `.genesis/perf/upgrade_plan_health_agent_inner_loop_report.json`
  - history: `.genesis/perf/upgrade_plan_health_agent_inner_loop_history.jsonl`
  - baseline seed history: `policies/perf/upgrade_plan_health_agent_inner_loop_seed_history.jsonl`
  - enforced by `scripts/check_upgrade_plan_health.sh --profile agent-inner-loop` via elapsed + history p95 wall-time gates.

## Runners

- Preferred runner: `cargo nextest` (configured by `/.config/nextest.toml`).
- Fallback runner: `cargo test` when nextest is unavailable.

## Local

- Default fast loop: `scripts/test_changed_fast.sh`
  - changed-file aware selection (or clean-tree fallback)
  - warms selfhost artifact cache when relevant paths change
  - emits deterministic metrics report `kind = genesis/test-changed-fast-metrics-v0.1`
  - default hard budget: 120000ms (`GENESIS_TEST_CHANGED_BUDGET_MS`)
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
  - all cargo-backed gates share one cache target dir (`GENESIS_HEALTH_CARGO_TARGET_DIR`,
    default `.genesis/build/health/<profile>`) to avoid cross-profile rebuild churn.
  - gate scheduler partitions cargo-backed commands from non-cargo commands and runs cargo lanes
    with dedicated shard control (`GENESIS_HEALTH_CARGO_GATE_SHARDS`, default `1`) to avoid
    lock contention while preserving full gate coverage.
  - defaults profile gates to serial execution (`GENESIS_HEALTH_PROFILE_SHARDS=1`) to reduce
    cargo build-lock contention while preserving full gate coverage
  - cargo prebuild orchestration is available via
    `GENESIS_HEALTH_WARM_CARGO_CACHE=auto|1|0` (default `auto`:
    `dev-fast=0`, `prepush-standard/release-full=1`) and reports to
    `.genesis/perf/upgrade_plan_health_warmup_<profile>.json`
    (`kind = genesis/upgrade-plan-health-cargo-warmup-v0.1`)
  - emits profile report `kind = genesis/upgrade-plan-health-profile-v0.1` at
    `.genesis/perf/upgrade_plan_health_profile_report.json`
  - enforces prepush wall-time budget `GENESIS_HEALTH_PREPUSH_BUDGET_MS` (default `1050000`)
    whenever health gates are enforced
  - GPU device-conformance lane policy:
    - `release-full` profile requires `scripts/check_gpu_compute_device_conformance.sh` by default.
    - `dev-fast` and `prepush-standard` remain opt-in via
      `GENESIS_HEALTH_REQUIRE_GPU_DEVICE_CONFORMANCE=1`.
  - Agent GPU automation profile contract:
    - automation contexts (`agent-inner-loop`, `prepush-standard`, `release-full`, `full-selfhost-cutover`)
      resolve an explicit `GENESIS_AGENT_GPU_PROFILE=agent-gpu-strict|agent-gpu-fallback`
      (caller-provided or profile-derived by `check_agent_reference_workflows.sh`).
    - strict profile (`agent-gpu-strict`) forces fail-closed policy:
      `GENESIS_HEALTH_GPU_BACKEND_POLICY_DEFAULT=require-device` and
      `GENESIS_GPU_COMPUTE_BACKEND_POLICY=require-device`; the gauntlet runtime also exports
      `GENESIS_GPU_BACKEND_POLICY_DEFAULT=require-device`.
    - fallback profile (`agent-gpu-fallback`) forces explicit fallback policy:
      `GENESIS_HEALTH_GPU_BACKEND_POLICY_DEFAULT=allow-fallback` and
      `GENESIS_GPU_COMPUTE_BACKEND_POLICY=dev-allow-fallback`; the gauntlet runtime also exports
      `GENESIS_GPU_BACKEND_POLICY_DEFAULT=allow-fallback`.
    - downgrade attempts (strict profile + fallback policy env) are rejected by
      `scripts/check_agent_gpu_profile_contract.sh`.
  - GPU/GFX decoupled runtime lanes:
    - compute-only lane: `scripts/check_gpu_compute_runtime_profile.sh`
    - gfx-only lane: `scripts/check_gfx_runtime_profile.sh`
  - release/full deployment target runtime lanes are fail-closed via:
    - `scripts/check_gcpm_target_runtime_pipelines.sh`
      (requires deterministic runtime runner bundle artifacts + `contract/boot/smoke` lane outputs
      for `ios|android|edge|service-runtime` targets).
  - release-full profile also enforces production WASM surface isolation:
    - `scripts/check_wasm_production_surface.sh` (forbids parity-only Rust frontend exports in default-feature wasm-bindgen artifacts).
  - high-churn Rust decomposition progress is fail-closed via:
    - `scripts/check_source_decomposition_progress.sh`
      (enforces target line budgets for tracked production modules).
- Agent authoring inner-loop: `scripts/check_upgrade_plan_health.sh --profile agent-inner-loop`
  - runs a narrowed deterministic contract set plus `cli_smoke` and changed-fast loop checks to reduce repeated process startup overhead.
  - enforces warm-cache budget `GENESIS_HEALTH_AGENT_INNER_LOOP_BUDGET_MS` (default `300000`) with history p95/min-history controls:
    - `GENESIS_HEALTH_AGENT_INNER_LOOP_MIN_HISTORY`
    - `GENESIS_HEALTH_AGENT_INNER_LOOP_REQUIRE_MIN_HISTORY`
    - `GENESIS_HEALTH_AGENT_INNER_LOOP_BASELINE_HISTORY`
- Full-selfhost closure lane: `scripts/check_upgrade_plan_health.sh --profile full-selfhost-cutover`
  - runs `scripts/check_full_selfhost_cutover_profile.sh` with strict refresh enabled.
  - enforces explicit closure-contract verification from `docs/spec/FULL_SELFHOST_CUTOVER_PROFILE_v0.1.md`.

### AI Iteration SLO Contention Policy

- `scripts/check_ai_iteration_slo.sh` enforces budgets using **median-of-samples** per metric,
  not single-shot wall time.
- Default sample counts are tuned for contention robustness without excessive loop time:
  - `incremental_warm_ms`: `GENESIS_AI_ITERATION_SLO_SAMPLES_INCREMENTAL_WARM=3`
  - `changed_fast_ms`: `GENESIS_AI_ITERATION_SLO_SAMPLES_CHANGED_FAST=2`
  - `core_suite_ms`: `GENESIS_AI_ITERATION_SLO_SAMPLES_CORE_SUITE=2`
  - `gcpm_lock_ms`: `GENESIS_AI_ITERATION_SLO_SAMPLES_GCPM_LOCK=2`
  - `gcpm_env_ms`: `GENESIS_AI_ITERATION_SLO_SAMPLES_GCPM_ENV=2`
- Reports include raw sample vectors + spread telemetry and contention warnings
  (`GENESIS_AI_ITERATION_SLO_CONTENTION_WARN_PERCENT`, default `60`).
- `gcpm lock/env` paths use deterministic warm-up + stabilization retries before final
  sample-window statistics:
  - `GENESIS_AI_ITERATION_SLO_WARMUP_GCPM_LOCK`
  - `GENESIS_AI_ITERATION_SLO_WARMUP_GCPM_ENV`
  - `GENESIS_AI_ITERATION_SLO_STABILIZE_RETRIES_GCPM_LOCK`
  - `GENESIS_AI_ITERATION_SLO_STABILIZE_RETRIES_GCPM_ENV`
- Baseline regression gates continue to use history p95, but compare against
  median-per-run metrics to reduce host contention noise.

### Perf Gate Disk-Headroom Strictness

- Perf-oriented gates now share one strictness selector:
  `GENESIS_PERF_DISK_STRICT_MODE=auto|1|0`.
- Default is `auto`, which delegates to `scripts/check_disk_headroom.sh`:
  - `CI=true` => strict fail-closed behavior.
  - local (`CI!=true`) => non-strict continuation after deterministic diagnostics.
- Affected gates:
  - `scripts/check_perf_budgets.sh`
  - `scripts/check_hot_path_budgets.sh`
  - `scripts/check_ai_iteration_slo.sh`
  - `scripts/check_runtime_microbench_budgets.sh`
- To force strict local behavior, set:
  `GENESIS_PERF_DISK_STRICT_MODE=1`.

### Bootstrap-Retirement Guard Disk Degraded Mode

- `scripts/check_bootstrap_retirement_gate.sh` remains strict/fail-closed in CI.
- Local constrained-disk environments can enable deterministic degraded mode:
  - `GENESIS_BOOTSTRAP_RETIREMENT_LOCAL_DEGRADED_MODE=1`
  - optional reclaim toggle: `GENESIS_BOOTSTRAP_RETIREMENT_DISK_AUTO_RECLAIM=0|1`
- Degraded runs are explicitly labeled and reported as non-pass:
  - report: `.genesis/perf/bootstrap_retirement_gate_report.json`
  - kind: `genesis/bootstrap-retirement-gate-report-v0.1`
  - status: `ok|degraded|fail`
- Degraded status is for local operator continuity only and cannot be used as release sign-off.

## CI Profiles

- `fast`: runs `scripts/test_changed_fast.sh` (default local/CI fast path)
- `standard|full`:
  - installs nextest
  - uses deterministic shard execution when `GENESIS_TEST_SHARDS_TOTAL > 1`
  - otherwise runs full workspace tests with nextest (`--cargo-profile selfhost-strict`)
  - preserves existing strict/smoke/golden gates as separate steps
  - runs `scripts/check_ai_stress_suite.sh` to enforce deterministic high-throughput stress
    coverage for tasks + bridge + gpu/compute + replay integrity.
  - runs `scripts/check_agent_reference_workflows.sh` as the scored
    agent-capability gauntlet (`genesis/agent-capability-gauntlet-v0.1`) with
    required domain thresholds for service, network/process, raw-network,
    inbound-server, durable-data, package-publish/sync, graphics, gpu/compute, filesystem,
    process-lifecycle, plugin-runtime, and time-control workflows.
  - runs `scripts/check_agent_scenario_perf.sh` for aggregated end-to-end scenario latency SLOs
    (median + p95 + regression policy) derived from gauntlet workflow durations.
  - runs `scripts/check_agent_generative_workloads.sh` for mutation-based workload validation
    beyond the fixed reference workflow list.
  - full release-profile workflows also require dual GPU conformance lanes
    (`gpu_device_microbench` + `gpu_device_microbench_deterministic`) and
    lane-contract parity via `scripts/check_gpu_device_conformance_lane_parity.sh`.
- Iteration conformance check:
  - `scripts/check_default_iteration_workflow.sh` validates measurable fast-path execution and
    deterministic shard selection.
  - default budget for changed-fast in this check is 120000ms (`GENESIS_BUDGET_CHANGED_FAST_MS`).

## Drift Guard

Profile/budget drift is blocked by:
- `scripts/check_test_execution_profile_matrix.sh`
- `scripts/check_cargo_target_dir_policy.sh` (compile-heavy cargo target-dir conformance)

This guard enforces:
- matrix rows for `smoke`, `changed-fast`, `strict-golden`, `full-cross-host`
- explicit budget strings in this spec
- CI step presence for each matrix lane
- 120000ms default budget pin for local high-signal workflows
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
