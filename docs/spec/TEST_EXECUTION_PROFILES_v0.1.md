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
| `perf-gate-regressions` | `bash scripts/test_perf_gates.sh` | profile-specific |
| `kernel-tail-stress` | `bash scripts/test_perf_gates.sh --kernel-tail-stress` | `<= 5m` |
| `persistent-sharing-stress` | `bash scripts/test_perf_gates.sh --test persistent_sharing_stress` | `<= 2m` |
| `agent-inner-loop` | `bash scripts/check_upgrade_plan_health.sh --profile agent-inner-loop` | `<= 5m` |
| `release-full` | `bash scripts/check_upgrade_plan_health.sh --profile release-full` | `<= 45m` |
| `strict-golden` | `bash scripts/selfhost_strict_golden.sh` | `<= 8m` |
| `full-cross-host` | strict golden + `node scripts/wasm_cross_host_determinism.mjs` + `bash scripts/check_full_cross_host_profile_budget.sh` | `<= 12m` |

The local default high-signal workflow is `changed-fast` with a hard 120000ms budget.

Release-hardening guard lanes:
- pinned host tools, feature profiles, platform SDK envelopes, and read-only diagnosis: `scripts/check_prerequisite_manifest.sh`; inspect with `scripts/genesis_prerequisites.sh --profile <id>`
- root workspace/lock identity: `scripts/check_root_lock_policy.sh` using a
  dependency-free POSIX `awk` parser with embedded duplicate/missing/type
  negative controls (no Python 3.11 requirement)
- capability-index freshness: `scripts/check_capability_indices.sh`
- roadmap execution graph drift/adversarial contract: `scripts/check_roadmap_execution_manifest.sh`
- explicit roadmap graph regeneration: `scripts/update_roadmap_execution_manifest.sh`
- in-toto/DSSE/SLSA evidence schema and authenticated-vector contract: `scripts/check_genesis_evidence_profile.sh`
- explicit evidence-vector regeneration: `scripts/update_genesis_evidence_profile.sh`
- standalone offline evidence verifier, trust policy, Merkle tree, and adversarial vectors: `scripts/check_genesis_evidence_verifier.sh`
- explicit verifier-vector regeneration: `scripts/update_genesis_evidence_verifier_vectors.sh`
- exact R0.2.e adversarial evidence/replay matrix: `scripts/check_evidence_adversarial_matrix.sh`
- E0-E4 storage authority, deterministic release archive, and create-new mirror contract: `scripts/check_evidence_storage_classes.sh`
- explicit fixture/release generation: `scripts/update_evidence_fixture_classification.sh`, `scripts/update_evidence_release_asset.sh`
- generated artifact source-control policy: `scripts/check_generated_artifact_policy.sh`
- warning-denied Rust policy: CI runs workspace/all-target Clippy, the runtime-backend matrix runs every supported mutually exclusive CLI/effect profile with warnings denied, and `scripts/lib/lint_suppression_policy.py` rejects module/workspace suppression while requiring a reason on any narrow item-level expectation
- version/changelog/selfhost metadata hygiene: `scripts/check_versioning_release_hygiene.sh`
- supply-chain policy: `scripts/check_supply_chain.sh` using `cargo-deny` and `deny.toml`
- release smoke contract: `scripts/check_release_smoke.sh`
- generated release-note contract: `scripts/check_release_notes.sh`; refresh only with `scripts/update_release_notes.sh`
- frozen agent authoring surface: `docs/spec/GC_AGENT_PROFILE_v0.3.json`, validated read-only by `scripts/check_gc_agent_profile.sh`; refresh only with `scripts/update_agent_authoring_bundle.sh profile`
- compact agent card: `docs/spec/GC_AGENT_CORE_CARD_v0.3.md` with machine manifest `docs/spec/GC_AGENT_CORE_CARD_v0.3.json`, validated read-only by `scripts/check_gc_agent_core_card.sh`; refresh only with `scripts/update_gc_agent_core_card.sh`
- intent-selected task cards: `docs/spec/GC_AGENT_TASK_CARDS_v0.3.md` with embedded registry `docs/spec/GC_AGENT_TASK_CARDS_v0.3.json`, validated read-only against production `agent-plan` by `scripts/check_gc_agent_task_cards.sh`; refresh only with `scripts/update_gc_agent_task_cards.sh`
- exact language-symbol index: `docs/spec/GC_AGENT_SYMBOL_INDEX_v0.3.json` under closed schema `docs/spec/GC_AGENT_SYMBOL_INDEX_v0.3.schema.json`, validated read-only against frozen authorities and production `agent-index --symbol` by `scripts/check_gc_agent_symbol_index.sh`; refresh only with `scripts/update_gc_agent_symbol_index.sh`
- canonical release docs: `CHANGELOG.md`, `docs/spec/VERSIONING_v0.1.md`, `docs/spec/RELEASE_SMOKE_v0.1.md`

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
  - validated read-only by `scripts/check_full_cross_host_profile_budget.sh` as the strict-golden + wasm-cross-host elapsed sum with a history p95 gate and fail-closed minimum-history enforcement.
  - retained only by `scripts/update_full_cross_host_profile_budget_report.sh`, after the strict-golden and wasm-cross-host producers have emitted their prerequisite reports.
- `runtime-workload-bench` evaluator workload lane
  - report: `.genesis/perf/runtime_workload_bench_report.json`
  - history: `.genesis/perf/runtime_workload_bench_history.jsonl`
  - runtime report: `.genesis/perf/runtime_workload_bench_runtime_report.json`
  - runtime history: `.genesis/perf/runtime_workload_bench_runtime_history.jsonl`
  - baseline seed history: `policies/perf/runtime_workload_bench_runtime_seed_history.jsonl`
  - validated read-only by `scripts/check_runtime_workload_budgets.sh` using `gc_runtime_bench --mode workloads`; retained only by `scripts/update_runtime_workload_budgets_report.sh`.
  - default `smoke` profile measures evaluator workloads with practical sample sizes and a representative selfhost parser corpus.
  - `GENESIS_RUNTIME_WORKLOAD_PROFILE=roadmap GENESIS_RUNTIME_WORKLOAD_REQUIRE_ROADMAP_SIZES=1` forces the full roadmap target sizes and the full `selfhost/parse.gc` + `prelude/prelude.gc` parser corpus.
  - `policies/perf/roadmap_workloads_v0.1.json` is the normative PB-1 through PB-10 workload authority. Existing scalar `best_of` reports are E0 diagnostics and cannot satisfy the normalized baseline protocol; R0.5.c must retain all raw samples, bind this policy identity and a conformant reference-host observation, apply the declared confidence rule, and sign the resulting evidence before a baseline claim is valid.
  - `scripts/check_roadmap_baseline.sh` validates the retained signed E0 baseline without recapturing it; `scripts/update_roadmap_baseline.sh` is the sole append-only producer. The independent verifier receives the raw public key and expected key ID out of band from the DSSE envelope, and successful fixture verification explicitly reports `signatureGrantsAuthority=false`.
  - `scripts/check_release_notes.sh` validates `docs/program/RELEASE_NOTES_v0.2.0.json` and the generated `CHANGELOG.md` block from canonical compatibility, migration, capability, evidence, dependency, and security inputs. Static notes remain E1 and list runtime gates as required-but-not-attested; only `scripts/update_release_notes.sh` may refresh them.
- `agent-scenario-perf` aggregate lane
  - report: `.genesis/perf/agent_scenario_perf_report.json`
  - history: `.genesis/perf/agent_scenario_perf_history.jsonl`
  - baseline seed history: `policies/perf/agent_scenario_perf_seed_history.jsonl`
  - enforced by `scripts/check_agent_scenario_perf.sh` from gauntlet component durations (service + durable-data + gfx-loop + network-process) with median + p95 + regression gates and fail-closed minimum-history enforcement.
  - every scenario seed is derived from, and checked against, the same four components in `policies/perf/agent_capability_gauntlet_seed_history.jsonl`; duplicated baseline drift fails closed.
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
  - hosted standard/full CI provisions the report and history once with
    `scripts/update_agent_reference_workflows_report.sh`, then persists scenario
    and generative reports before artifact upload. Standard CI records a declared
    single-native-profile generative cohort with secondary parity disabled; full
    CI first runs `scripts/update_agent_workflow_runtime_parity_report.sh` and
    requires the resulting native/WASI pair for generative validation. Reports
    from these unlike runtime-profile cohorts never share ranking or baselines.
- `agent-inner-loop` health lane
  - report: `.genesis/perf/upgrade_plan_health_agent_inner_loop_report.json`
  - history: `.genesis/perf/upgrade_plan_health_agent_inner_loop_history.jsonl`
  - baseline seed history: `policies/perf/upgrade_plan_health_agent_inner_loop_seed_history.jsonl`
  - enforced by `scripts/check_upgrade_plan_health.sh --profile agent-inner-loop` via elapsed + history p95 wall-time gates.
- `large-workspace-agent-perf` release lane
  - report: `.genesis/perf/large_workspace_agent_perf_report.json`
  - runtime report: `.genesis/perf/large_workspace_agent_runtime_report.json`
  - runtime history: `.genesis/perf/large_workspace_agent_runtime_history.jsonl`
  - validated read-only by `scripts/check_large_workspace_agent_perf.sh` with a generated `>=10000` module workspace and hard budgets for:
    - `gcpm lock`
    - `gcpm build`
    - `gcpm test`
    - `selfhost-artifact` refresh
  - retained only by `scripts/update_large_workspace_agent_perf_report.sh`.

## Runners

- Preferred runner: `cargo nextest` (configured by `/.config/nextest.toml`).
- Fallback runner: `cargo test` when nextest is unavailable.

## Local

- Default fast loop: `scripts/test_changed_fast.sh`
  - selection is governed by `policies/changed_impact_v0.1.json` and checked by
    `scripts/check_changed_impact.sh`
  - computes Cargo reverse-dependency and gate-manifest impact closures
  - includes committed, staged, unstaged, deleted, renamed, and untracked paths
  - escalates schemas, generated views, unknown paths, and oversized/ambiguous
    selections to `prepush-standard` rather than guessing a narrower target
  - warms selfhost artifact cache when relevant paths change
  - emits `kind = genesis/test-changed-fast-metrics-v0.1` into a private temporary
    report/history pair by default; retain local E0 timing history only through
    `scripts/update_test_changed_fast_metrics.sh`
  - targeted and clean-tree hard budget: 120000ms (`GENESIS_TEST_CHANGED_BUDGET_MS`)
  - targeted selection is reserved for genuinely narrow impact closures, such as a
    single leaf gate implementation; Rust/source changes whose crate or direct-gate
    fan-out exceeds the policy ceilings escalate to `prepush-standard`
  - automatically escalated `prepush-standard` selections use the existing GB-3
    720000ms/3 GiB envelope (`GENESIS_TEST_CHANGED_FALLBACK_BUDGET_MS`); an explicit
    `--budget-ms` or `GENESIS_TEST_CHANGED_BUDGET_MS` remains authoritative and is
    never silently widened
  - measures additional disk as allocated-block growth in the loop's active
    content-addressed Cargo target; isolated generated-authority worktrees keep
    path-specific Cargo products in their bounded transient stage and reclaim
    them with that stage. Unrelated host allocation and concurrent builds in
    other cache identities cannot consume the active 1 GiB or escalated 3 GiB
    residual allowance
- Alias wrapper: `scripts/test_fast.sh`
  - defaults to `scripts/test_changed_fast.sh`
  - pass `--full` to run the broad fast suite
- Full fast fallback: `scripts/test_fast_full.sh`
  - auto-detects nextest
  - runs high-signal core libs + selected CLI integration tests
- Full/sharded loop: `scripts/test_shard_workspace.sh --total N --index I --runner auto|nextest|cargo`
  - deterministic shard assignment by `(seed, crate)` hash
  - emits report `kind = genesis/test-shard-report-v0.1`
- Default `cargo test --workspace` contract:
  - must not execute repo-level `scripts/check_*.sh` gates, perf/SLO loops, or nested cargo workflows.
  - integration tests that exercise those lanes are marked `#[ignore = "perf-gate"]`.
  - run ignored gate regression tests explicitly with `scripts/test_perf_gates.sh`.
  - `scripts/test_perf_gates.sh` runs ignored targets serially via `cargo test -p gc_cli --test <target> -- --ignored --test-threads=1`, using the declared `root-host` content-addressed Cargo cache scope.
  - standard CI runs all ignored targets with `GENESIS_HEALTH_PROFILE=dev-fast`; this
    keeps the nested aggregate-health regression non-recursive while retaining the
    dedicated scorer, stress, SLO, bridge, package, and runtime targets. Full CI runs
    the same target set with `GENESIS_HEALTH_PROFILE=release-full` and must provide
    every authentic release-only runtime prerequisite. Standard CI never substitutes
    synthetic target evidence for release qualification.
  - the exhaustive GenesisBench scorer reference/adversarial matrices run only as the required serial `cli_agent_benchmark_scoring` perf target. The default lane retains one accepted-reference execution plus candidate-root and file-symlink rejection. Every scorer child has the scoring contract's `30000ms` hard process-group timeout, and the complete matrix has a `600000ms` measured ceiling configurable only to a positive value through `GENESIS_SCORING_MATRIX_BUDGET_MS`.
- Prepush strict loop: `scripts/check_upgrade_plan_health.sh --profile prepush-standard`
  - the check executes the aggregate profile with private temporary reports and copied input-only history; it ignores legacy retained-output environment variables and disables persistent gate-result caching.
  - retain profile, history, warmup, and disk-preflight observations only with `scripts/update_upgrade_plan_health_report.sh --profile <profile>`.
  - defaults to deterministic gate sharding (`GENESIS_HEALTH_SHARDS`) derived from host parallelism
    for non-release loops (2-way on small hosts, 4-way on larger hosts)
  - all cargo-backed gates resolve the same content-addressed `root-host` cache;
    health profile and gate names never participate in the directory identity.
  - gate scheduler partitions cargo-backed commands from non-cargo commands and runs cargo lanes
    with dedicated shard control (`GENESIS_HEALTH_CARGO_GATE_SHARDS`, default `1`) to avoid
    lock contention while preserving full gate coverage. Release-pair workers retain exactly one
    Cargo gate shard because performance, stress, and evidence-producing gates require exclusive
    execution and share a pair-local content-addressed target. Pair workers remain independent CI
    jobs with disjoint caches. Within each pair, source-decomposition parity validates the sealed
    parity evidence produced by the serialized setup gate instead of rerunning the native/WASI
    gauntlets; manifest-bound reuse removes duplicate work without overlapping benchmark lanes.
  - `release-full` first runs `scripts/render_health_profile_evidence_bundle.sh` as a serialized
    setup gate, before common gates can expand shared build caches. It renders gauntlet,
    native/WASI parity, generative, runtime-backend, host-bridge,
    WebXR/GPU-XR, and assurance reports under one private temporary root, validates their kinds and
    `ok` states into a hash manifest, and binds every parallel consumer to those explicit paths.
    Untracked `.genesis/perf` reports are never release inputs; targeted gate overrides skip setup.
    The bundle keeps the runtime-backend matrix at `360000ms` for `prepush-standard` and uses the
    gate-manifest `600000ms` envelope only for the empty-target, non-incremental `release-full`
    build. This subordinate bound does not alter the per-profile GB-4 `2700000ms` ceiling.
    Its primary `release-full` gauntlet is also the native parity lane; the bundle copies those
    exact bytes into the parity-owned artifact name, executes only the missing WASI lane, and
    requires the parity producer to validate both reports as fresh, same-profile evidence before
    reuse. Each independent release pair runs three task-concurrency and three host-bridge stress
    repetitions, yielding odd per-pair decisions and six total repetitions across the two pairs.
    The governed runtime-backend check entrypoint validates its bundle-local prebuilt report
    against the direct-sibling bundle manifest, release profile, kind, and SHA-256 before avoiding
    a repeated compilation matrix.
    WebXR evidence resolves Node.js 22.x as declared by `genesis.prerequisites.json`;
    `GENESIS_WEBXR_NODE_BIN` may select an explicit compatible executable.
  - defaults profile gates to serial execution (`GENESIS_HEALTH_PROFILE_SHARDS=1`) to reduce
    cargo build-lock contention while preserving full gate coverage
  - deterministic heavy-gate cache policy for warm loops:
    - enabled by default for `prepush-standard` via `GENESIS_HEALTH_PROFILE_GATE_CACHE=auto|1`
      (default `auto` resolves to `1` on `prepush-standard`, `0` otherwise)
    - cache keys are content-fingerprinted from gate command + gate-specific input path sets
      and stored under `.genesis/perf/health_gate_cache/<profile>/`
    - TTL-bound reuse controlled by `GENESIS_HEALTH_PROFILE_GATE_CACHE_TTL_SEC`
      (default `21600`, six hours)
    - implementation wrapper: `scripts/lib/run_cached_health_gate.sh`
  - cargo prebuild orchestration is available to the explicit renderer/updater via
    `GENESIS_HEALTH_WARM_CARGO_CACHE=auto|1|0` (default `auto`:
    `dev-fast/agent-inner-loop=0`, `prepush-standard/release-full=1`) and reports to
    `.genesis/perf/upgrade_plan_health_warmup_<profile>.json`
    (`kind = genesis/upgrade-plan-health-cargo-warmup-v0.1`)
  - the explicit updater emits profile report `kind = genesis/upgrade-plan-health-profile-v0.1` at
    `.genesis/perf/upgrade_plan_health_profile_report.json`
  - enforces prepush wall-time + history p95 budget
    `GENESIS_HEALTH_PREPUSH_BUDGET_MS` (default `720000`, the GB-3 twelve-minute ceiling)
    via `scripts/lib/profile_runtime_budget.py` using:
    - `GENESIS_HEALTH_PREPUSH_HISTORY`
    - `GENESIS_HEALTH_PREPUSH_MIN_HISTORY`
    - `GENESIS_HEALTH_PREPUSH_REQUIRE_MIN_HISTORY`
    - `GENESIS_HEALTH_PREPUSH_BASELINE_HISTORY`
    - `GENESIS_HEALTH_PREPUSH_HISTORY_SCOPE_KEY`
  - fails closed when the current prepush sample is absent, exceeds twelve minutes,
    or adds more than 3 GiB of generated disk; retained history remains input-only
    to checks.
  - excludes the closed `releaseFullOnlyGates` inventory in
    `policies/engineering_gate_budgets_v0.1.json`; the engineering-budget contract
    proves those gates are absent from common/prepush scheduling and present in
    `release-full`.
  - panic assurance is split intentionally: `scripts/check_no_user_panics.sh`
    is the compiler-free GB-1 source/policy gate, while
    `scripts/check_no_user_panics_compiler.sh` retains the Clippy semantic lane
    in `prepush-standard`, `release-full`, and standard/full CI.
  - enforces release-full wall-time + history p95 budget
    `GENESIS_HEALTH_RELEASE_FULL_BUDGET_MS` (default `2700000`)
    via `scripts/lib/profile_runtime_budget.py` using:
    - `GENESIS_HEALTH_RELEASE_FULL_HISTORY`
    - `GENESIS_HEALTH_RELEASE_FULL_MIN_HISTORY`
    - `GENESIS_HEALTH_RELEASE_FULL_REQUIRE_MIN_HISTORY` (default `1`, fail-closed)
    - `GENESIS_HEALTH_RELEASE_FULL_BASELINE_HISTORY` (default `policies/perf/upgrade_plan_health_release_full_seed_history.jsonl`)
    - `GENESIS_HEALTH_RELEASE_FULL_HISTORY_SCOPE_KEY`
  - R0.4.j timing calibration is governed by
    `policies/engineering_gate_timing_calibration_v0.1.json`, validated through
    the existing `scripts/check_engineering_gate_contract.sh`, and retained in
    `docs/program/ENGINEERING_GATE_TIMING_CALIBRATION_v0.1.json` under the closed
    `docs/spec/ENGINEERING_GATE_TIMING_CALIBRATION_v0.1.schema.json` schema.
    Raw observations use the separately closed
    `docs/spec/ENGINEERING_GATE_TIMING_OBSERVATION_v0.1.schema.json` envelope.
    The authority keeps `local-warm`, `local-clean-fallback`, and
    `hosted-cold-shared-runner` as three ordered, non-interchangeable classes.
    Each observation binds its exact commit, host, toolchain, workload,
    cache-precondition, competing-lane, and typed source identities; duplicate
    sequence/source identities, cache relabeling, and undeclared outcomes fail.
  - The two local classes execute one exact policy-owned workload:
    `scripts/test_changed_fast.sh --base HEAD --runner cargo --min-history 1`
    in `profile-fallback` mode from the declared one-file impact input. Collection
    requires clean `main` at exact `origin/main`, a conformant reference host,
    nominal thermal state, bounded background load, no competing build process,
    an exclusive advisory collector lock, process-group timeout/kill/reap, and a
    report proving the selected profile, runner, file count, budget, and terminal
    result. `local-warm` requires the reusable root-host cache; the clean class
    uses the updater's fresh external worktree and a collector-owned empty Cargo
    cache. The collector is:
    `python3 scripts/lib/engineering_gate_timing_observations.py record-local --class-id <local-warm|local-clean-fallback>`.
  - The hosted class measures the exact `test_suite` standard job only for an
    explicit `workflow_dispatch` of `main`. `Begin Hosted Timing Calibration`
    starts the monotonic interval immediately after checkout; `Finalize Hosted
    Timing Calibration` closes it after native/WASI smoke; the resulting closed
    observation is uploaded as a run-attempt-scoped artifact. Floating refs,
    non-standard profiles or lanes, non-Linux/x86-64 hosts, non-ubuntu24 runner
    images, and missing start envelopes fail closed.
  - Local JSONL history and logs under `.genesis/perf/` are ignored, append-only,
    hash-chained E0 observations. Hosted artifacts are likewise observations,
    not authority. `render-candidate` validates every supplied observation,
    retains failures in chronology, assigns the first five successful records
    to warmups and all later successful records to the retained population, and
    emits an E0 review candidate without editing policy or canonical evidence.
    Promotion is a separate reviewed transaction. Canonical samples preserve the
    exact host, toolchain, control, timestamp, terminal, and chain material and
    recompute the originating observation identity, so review remains possible
    after mutable collection storage is gone.
  - A class remains `collecting` until exactly five semantic-pass warmups have
    been discarded and at least 30 semantic-pass measurements are retained.
    The first 30 retained measurements form the immutable calibration population:
    exact-rational median and MAD, nearest-rank p95, and the distribution-free
    rank-10/rank-21 median interval are independently recomputed. Additional
    conformant measurements and every hard failure remain retained. The most
    recent 30 measurements form the current trend window; once 60 exist, the
    prior and current windows are compared with separate 10% median and p95
    alarms.
  - A calibrated hard ceiling is the nearest 1,000ms above p95 plus the larger
    of six MADs or 10% proportional headroom. Completion requires that value to
    remain inside the prior containment ceiling, be copied exactly into a
    `ratified` class policy, and pass the verifier. Until then the existing
    720,000ms local and 7,200,000ms hosted values are provisional containment
    limits only. A semantic pass cannot override a timing overrun, collection
    cannot authorize a budget increase, and neither mutable E0 observations nor
    a `collecting` evidence file can close R0.4.j or qualify a release.
  - strict profiles (`prepush-standard`, `release-full`, `full-selfhost-cutover`)
    fail closed on low-disk preflight by default
    (`GENESIS_HEALTH_STRICT_DISK_POLICY=fail`)
  - The supply-chain gate uses the exact RustSec commit and canonical tree
    identity in `policies/rustsec_advisory_db_v0.1.json`. CI prepares that
    snapshot in the declared dependency-network phase; the gate verifies the
    installed commit, clean Git tree, content identity, file/byte bounds, and
    licenses before invoking
    `cargo deny --locked --offline check --disable-fetch`; subsequent Cargo
    metadata inspection is also locked and offline.
    Floating advisory HEAD, gate-time fetching, a stale or dirty checkout, and
    a host-global advisory database fail closed. Yanked crates are denied rather
    than downgraded to cargo-deny's default warning. Updating the pin is a
    reviewed policy transaction and never occurs inside a check.
  - The standard hosted regression lane retains the 360,000 ms local warm-cache
    target but uses a provisional 485,000 ms cold nested-health containment
    envelope. Thirty successful exact-revision hosted observations had a
    nearest-rank p95 of 439,553 ms and MAD of 15,059/2 ms; the declared
    p95-plus-max(6*MAD, 10%) rule yields 484,730 ms and rounds upward to
    485,000 ms. Runs 31308665169 (451,394 ms) and 31314057937 (452,351 ms)
    remain timing failures under the superseded 450,000 ms envelope; this
    adjustment does not relabel them, ratify a performance SLO, change GB-3, or
    substitute for the complete section 3.21 class calibration.
  - GPU device-conformance lane policy:
    - `release-full` renders current real-device and deterministic-device conformance into
      one private temporary evidence root and requires lane parity by default.
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
    - hosted CI applies this contract during profile resolution and persists all three
      resolved backend-policy variables through `GITHUB_ENV`; declaring
      `GENESIS_AGENT_GPU_PROFILE` without applying its policy is invalid.
  - GPU/GFX decoupled runtime lanes:
    - compute-only lane: `scripts/check_gpu_compute_runtime_profile.sh`
    - gfx-only lane: `scripts/check_gfx_runtime_profile.sh`
  - evaluator workload perf lane:
    - `scripts/check_runtime_workload_budgets.sh` renders workload metrics and a wall-time profile into a private temporary root.
    - `scripts/update_runtime_workload_budgets_report.sh` is the sole retained-evidence producer used by CI artifact collection.
    - perf PRs that change evaluator hot paths must update workload budgets and history evidence in the same change.
  - release/full deployment target runtime lanes are fail-closed via:
    - `scripts/check_gcpm_target_runtime_pipelines.sh`
      (renders deterministic runtime runner bundle artifacts + `contract/boot/smoke`
      lane outputs under a private temporary root for
      `ios|android|edge|service-runtime` targets).
    - `scripts/update_gcpm_target_runtime_pipelines_report.sh` is the sole retained
      `.genesis/perf/gcpm_target_runtime_evidence_report.json` and replay-artifact producer.
    - strict non-synthetic policy:
      - `GENESIS_GCPM_TARGET_RUNTIME_REQUIRE_NON_SYNTHETIC=1` requires a typed authentic lifecycle or a typed readiness blocker for every selected target.
      - default strictness follows CI context (`CI=true` => strict).
      - `policies/release_target_reference_set_v0.1.json` binds the four named reference shards, command and identity inputs, runtime classes, and expected outcome.
      - `scripts/prepare_release_target_reference.sh` must successfully boot and identify the
        named iOS/Android simulator or probe the installed Wasmtime/container runtime before a
        hosted shard may set `GENESIS_GCPM_TARGET_RUNTIME_REQUIRE_REFERENCE_SETUP=1`.
      - while the product matrix remains unsupported, release-full sets `GENESIS_GCPM_TARGET_RUNTIME_EXPECT_OUTCOME=unsupported-product`; the runner does not execute configured runtime commands and records `release_qualified=false`.
      - `qualified` requires a matching `genesis/target-runtime-lifecycle-v0.1` record with install, launch, smoke, teardown, and reap success. Setup, infrastructure, execution, and unsupported-product states remain distinct.
  - release/full evidence reuse is closed by this contract and `docs/spec/HEALTH_PROFILE_EVIDENCE_BUNDLE_v0.2.schema.json`:
    - one `genesis/health-profile-evidence-bundle-v0.2` manifest binds source, environment, toolchain, producer, report/history, freshness, and consumer identities;
    - every prebuilt consumer validates the exact manifest and artifact set at its report-read boundary;
    - native/WASI gauntlets, generative parity, GPU/XR aggregation, host-bridge fault injection, and runtime-backend compilation execute once per profile rather than once per consumer.
  - GB-4 qualification is measured only by
    `scripts/measure_release_evidence_v02.sh` and
    `scripts/lib/release_evidence_execution.py`, under the closed
    `docs/spec/RELEASE_EVIDENCE_WORKER_v0.2.schema.json` observation and
    `docs/spec/RELEASE_EVIDENCE_AGGREGATE_v0.2.schema.json` decision contracts:
    - the retained worker kind is
      `genesis/release-evidence-worker-observation-v0.2`; its closed top level has
      no `ok`, `status`, `verdict`, qualification, or promotion field;
    - three independently scheduled cold cache-sensitive workers and three
      independently scheduled warm cache-sensitive workers form matched odd cohorts.
      The invariant class executes exactly once, and three independently isolated
      stress/performance workers form its odd cohort. The aggregate rejects any
      missing, duplicated, even, relabeled, or cross-class execution;
    - every worker owns a nonce-bound external ephemeral root, retains byte-bound
      profile reports and bounded logs, samples process-tree peak RSS and
      non-overlapping artifact roots, and proves complete root removal. Stress workers
      use exclusive identities, and the aggregate rejects any reused isolation nonce;
    - each warm worker starts with an empty owned root and executes the exact
      cache-sensitive command set as an unmeasured precondition under a proved kernel
      network-denial backend. Source, toolchain, feature-set, cache-key, and artifact
      inventory identities must remain equal at measured start. Only the subsequent
      setup and command phases contribute to the 2,700,000ms measured-worker ceiling;
    - cold sample 1 splits its measured setup and command phases at the workflow
      boundary. After setup produces the sole health-evidence bundle, the workflow
      publishes it under a name bound to run id, attempt, and revision; cold sample 1
      then completes its remaining measured commands. This split executes
      `setup/evidence-bundle` once and permits dependent workers to proceed without a
      serial producer job;
    - invariant and stress workers query only the current workflow run, require exactly
      one unexpired artifact with the canonical name, verify GitHub's archive digest,
      safely extract it under the 20GiB limit, and bind its manifest, DAG, source,
      producer class, cold-sample index, and exact profile, architecture, operating
      system, and toolchain inventory. Each manifest retains and authenticates its
      complete observed execution environment, but the hosted-runner kernel release is
      not a cross-worker compatibility field because GitHub does not pin one kernel
      build across distinct `ubuntu-24.04` workers. The renderer reauthenticates
      that sidecar, the aggregate matches every consumer digest to cold sample 1's
      service-issued upload receipt, and every evidence consumer rechecks its exact
      manifest inputs;
    - all ten measured workers have 55-minute job envelopes and 45-minute measured
      ceilings. Each initializes a run/attempt/revision-bound start observation before
      fanout or measurement, and its `always()` upload retains bounded orchestration
      and child diagnostics even when setup never begins. A read-only five-minute
      aggregate follows; producer reports cannot authorize the final result;
    - the aggregate requires exact command coverage from the v0.2 DAG, one workflow
      run/attempt/revision, distinct isolation identities, same-run cold-1 fanout
      custody, byte-exact artifacts, complete cleanup, and the exact named-target
      dispositions. It derives cold/warm p95 wall, peak-RSS, and artifact observations
      and alone emits `genesis/release-evidence-aggregate-v0.2 status=pass`;
    - R2.2.f host-handle closure is a separate derived field inside that aggregate, not
      a worker verdict. Cold sample 1 produces the authenticated Linux x86_64
      three-run host-fault matrix; only stress sample 1 retains its byte-exact report
      from the same-run fanout. The `macos-15` iOS shard places the Darwin arm64 public
      warm-daemon report inside its authenticated replay inventory. The aggregate
      retains both producer records and both reports, then independently re-derives
      lifecycle-path, resource-family, process-tree, child-reap, I/O-quiescence,
      daemon restart/drain, cleanup-bound, negative-control, host-diversity, probe-source,
      and self-host-artifact predicates. Missing custody, a producer-authored closure
      boolean, cross-run input, identity drift, a surviving process, or an omitted
      lifecycle fact fails closed. The resulting `hostHandleLifecycle` field may set
      `r2_2_f_closeable=true`; it does not standardize the R5.4.e model API or qualify
      an unsupported platform pack;
    - the producer requires one CI-provenanced report from every named target shard, with
      the exact runner label, complete reference shard, product-matrix limitation, build and
      runtime-log identities, source commit, and shared workflow run attempt. Those expected blockers keep
      `productReleaseQualified = false`, `profileOperational = true`, and
      `readinessStatus = unsupported-product`; they prove profile operation and readiness
      classification, not product support;
    - scheduled and manually dispatched full CI start all ten workers concurrently with
      the named-target shards. Hosted evidence workers run on `ubuntu-24.04`, the iOS
      readiness shard remains on `macos-15`, and the aggregate joins every branch on
      `ubuntu-24.04`. The required `test` disposition binds the
      aggregate result for full runs and accepts only `skipped|success` outside full runs.
      Release-evidence-owned report producers run only in the v0.2 DAG during full CI;
      the ordinary `test_suite` matrix retains those direct updater steps only for
      standard CI. The topology gate rejects reintroducing duplicate full-profile
      runtime-backend, performance, agent-gauntlet/parity, scenario, or generative
      producers while preserving their exact DAG selectors and coverage.
      The generic ignored-perf lane excludes `upgrade_plan_health`, preventing nested
      duplicate execution while retaining every other required perf target.
  - The v0.2 migration authority is `policies/release_evidence_dag_v0.2.json`, validated
    against the complete release command inventory by
    `scripts/lib/release_evidence_dag.py` and `scripts/check_test_execution_profile_matrix.sh`.
    It partitions every setup, common, profile, and device-conformance command into exactly
    one `cache-sensitive`, `invariant`, `stress-performance`, or explicitly superseded
    disposition. Superseded commands name a live replacement and cannot execute in a v0.2
    node. Class-selective health runs set `GENESIS_RELEASE_EVIDENCE_NODE_CLASS`; invariant
    and stress workers require an authenticated same-run bundle through
    `GENESIS_RELEASE_EVIDENCE_INPUT_ROOT` and `GENESIS_RELEASE_EVIDENCE_FANOUT_TOKEN`, while
    only a cache-sensitive worker may export a bundle through
    `GENESIS_RELEASE_EVIDENCE_EXPORT_ROOT`. Partial node reports bind the DAG identity and
    selected-command identity and cannot be treated as a complete monolithic profile.
    Until the measurement worker, aggregate schemas, and CI fan-out are promoted together,
    hosted full CI remains on the v0.1 pair topology and cannot claim v0.2 evidence.

### Closed Release Evidence Semantics

The `genesis/health-profile-evidence-bundle-v0.2` manifest binds every report and
history by kind, evidence class, bytes, and SHA-256; every producer by command,
declared environment, complete-input identity, source snapshot, OS/architecture, and
toolchain executable/version identities; and every authorized consumer by script,
profile, artifact set, and evidence class. It also binds generation, an exact six-hour
expiry window, maximum age, and a canonical content identity.

Every release gate reading bundle evidence sets
`GENESIS_HEALTH_EVIDENCE_REQUIRED=1`, provides
`GENESIS_HEALTH_EVIDENCE_MANIFEST`, and calls
`genesis_verify_health_profile_evidence` at its report-read boundary. Verification
rejects unknown fields, stale or future evidence, changed source or toolchain inputs,
unauthorized consumers, incomplete artifact sets, non-sibling paths, and changed
artifact bytes. Failure requires a producer rerun and must never fall back to ambient
`.genesis/perf` state.

The named target reference set binds runner class, product claim, authentic command,
runtime identity probe, SDK/image/device identity probe, and lifecycle. Strict target
states are `qualified`, `unsupported-product`, `setup-required`,
`infrastructure-failure`, and `execution-failure`; `synthetic-only` is a development
state and never qualifies. A lifecycle must bind target, bundle and package hashes,
runtime and SDK identities, and successful install, launch, smoke, teardown, and reap.
While `R6.3.f` and `R6.6` remain open, the product matrix forces
`unsupported-product`, configured runtime commands are not executed, and
`release_qualified` remains false even when the orchestrator correctly accepts the
expected readiness classification. Hosted reference reports additionally require non-empty
runtime and SDK/image/device identities from the prepared infrastructure; a runner label alone
is not reference evidence.
  - release-full profile also enforces production WASM surface isolation:
    - `scripts/check_wasm_production_surface.sh` (forbids parity-only Rust frontend exports in default-feature wasm-bindgen artifacts).
  - release-full profile also enforces large-workspace SLO coverage:
    - `scripts/check_large_workspace_agent_perf.sh` (`>=10000` generated modules; `gcpm lock/build/test` + `selfhost-artifact` refresh budgets).
  - high-churn Rust decomposition progress is fail-closed via:
    - `scripts/check_source_decomposition_progress.sh`
      (enforces target line budgets for tracked production modules).
    - `scripts/check_source_decomposition_tracked_parity.sh`
      (executes every tracked-row parity gate and enforces bounded, non-expired waiver contract metadata for over-budget modules; retained only by `scripts/update_source_decomposition_tracked_parity_report.sh`).
- Agent authoring inner-loop: `scripts/check_upgrade_plan_health.sh --profile agent-inner-loop`
  - runs a narrowed deterministic contract set plus `cli_smoke` and changed-fast loop checks to reduce repeated process startup overhead.
  - enforces warm-cache budget `GENESIS_HEALTH_AGENT_INNER_LOOP_BUDGET_MS` (default `300000`) with history p95/min-history controls:
    - `GENESIS_HEALTH_AGENT_INNER_LOOP_MIN_HISTORY`
    - `GENESIS_HEALTH_AGENT_INNER_LOOP_REQUIRE_MIN_HISTORY`
    - `GENESIS_HEALTH_AGENT_INNER_LOOP_BASELINE_HISTORY`
  - default fail-closed history floor: `GENESIS_HEALTH_AGENT_INNER_LOOP_MIN_HISTORY=5`.
- Full-selfhost closure lane: `scripts/check_upgrade_plan_health.sh --profile full-selfhost-cutover`
  - runs `scripts/check_full_selfhost_cutover_profile.sh` in read-only mode against
    explicitly produced prerequisite evidence.
  - enforces explicit closure-contract verification from `docs/spec/FULL_SELFHOST_CUTOVER_PROFILE_v0.1.md`.
  - is the only health profile that claims complete cutover. `agent-inner-loop`,
    `prepush-standard`, and the current `release-full` profile retain the common strict
    self-host boundary, dashboard, readiness, production-frontend-isolation, and parity gates,
    but cannot consume retained workstation reports or imply that the later cutover milestone
    is already complete.

### AI Iteration SLO Contention Policy

- `scripts/check_ai_iteration_slo.sh` validates budgets read-only using
  **median-of-samples** per metric, not single-shot wall time. Retained metrics
  and bounded history are produced only by `scripts/update_ai_iteration_slo_report.sh`.
- Default sample counts are tuned for contention robustness without excessive loop time:
  - `incremental_warm_ms`: `GENESIS_AI_ITERATION_SLO_SAMPLES_INCREMENTAL_WARM=3`
  - `changed_fast_ms`: `GENESIS_AI_ITERATION_SLO_SAMPLES_CHANGED_FAST=3`
  - `core_suite_ms`: `GENESIS_AI_ITERATION_SLO_SAMPLES_CORE_SUITE=3`
  - `gcpm_lock_ms`: `GENESIS_AI_ITERATION_SLO_SAMPLES_GCPM_LOCK=3`
  - `gcpm_env_ms`: `GENESIS_AI_ITERATION_SLO_SAMPLES_GCPM_ENV=3`
- Every median cohort is an odd sample count of at least three. This gives the
  decision statistic non-zero single-outlier resistance; an even two-sample
  arithmetic midpoint is rejected rather than mislabeled as a robust median.
- Reports include raw sample vectors + spread telemetry and contention warnings
  (`GENESIS_AI_ITERATION_SLO_CONTENTION_WARN_PERCENT`, default `60`).
- `gcpm lock/env` paths use deterministic warm-up + stabilization retries before final
  sample-window statistics:
  - `GENESIS_AI_ITERATION_SLO_WARMUP_GCPM_LOCK`
  - `GENESIS_AI_ITERATION_SLO_WARMUP_GCPM_ENV`
  - `GENESIS_AI_ITERATION_SLO_STABILIZE_RETRIES_GCPM_LOCK`
  - `GENESIS_AI_ITERATION_SLO_STABILIZE_RETRIES_GCPM_ENV`
- Baseline regression gates continue to use history p95, but compare against
  median-per-run metrics to reduce host contention noise. Baseline rows are
  scoped by report kind, build mode/profile/target, and the exact budget map;
  samples from unlike build profiles cannot tighten or relax another profile.
- The local and `full` release `changed_fast_ms` target remains 15000ms. Hosted
  `standard` CI declares a 20000ms contention envelope through
  `GENESIS_BUDGET_CHANGED_FAST_MS`; this does not alter the local/full target,
  and its reports cannot share baseline rows with the 15000ms budget map.

### Performance Evidence Lifecycle

- `scripts/check_hot_path_budgets.sh`, `scripts/check_perf_budgets.sh`,
  `scripts/check_runtime_workload_budgets.sh`, and
  `scripts/check_ai_iteration_slo.sh` execute the real budget workload but
  render reports and appended samples only under a private temporary root.
- Each read-only check may consume the corresponding retained history as an
  input-only p95 baseline. Caller-controlled producer output variables are not
  accepted by the check surface.
- Retention is explicit through `scripts/update_hot_path_budgets_report.sh`,
  `scripts/update_perf_budgets_report.sh`, and
  `scripts/update_runtime_workload_budgets_report.sh`, plus
  `scripts/update_ai_iteration_slo_report.sh`. CI uses these producers before
  uploading `.genesis/perf` trend artifacts.
- Renderers require caller-owned output and baseline paths. This keeps one
  implementation for check and update behavior while making persistence an
  auditable command-level decision.

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
  - checks never reclaim automatically; `GENESIS_BOOTSTRAP_RETIREMENT_DISK_AUTO_RECLAIM=1`
    is rejected with the explicit two-phase deterministic cleanup remediation
    in `docs/spec/CHECK_UPDATE_BOUNDARY_v0.1.md#deterministic-cleanup`.
- Degraded runs are explicitly labeled and reported as non-pass:
  - report: `.genesis/perf/bootstrap_retirement_gate_report.json`
  - kind: `genesis/bootstrap-retirement-gate-report-v0.1`
  - status: `ok|degraded|fail`
- Degraded status is for local operator continuity only and cannot be used as release sign-off.

## CI Profiles

- Feature branches are validated by the `pull_request` event only; direct `push`
  validation is restricted to canonical `main`. Pull-request runs share one
  concurrency group per pull request and cancel superseded commits, preventing
  duplicate cold-cache work without narrowing the selected gates. The resulting
  `main` push still runs the independent post-merge `fast` profile. Pushes bind
  their immutable commit SHA, while schedules and dispatches bind their unique run
  identity; those event classes never share a pending slot and therefore cannot
  silently replace one another under GitHub's one-pending-run concurrency rule.
- `policies/ci_control_plane_v0.1.json` is the closed authority for CI workflow
  paths, the canonical branch, exact self-hosted label sets, hardware-pack
  selections, and liveness limits. Scheduled full runs select the optional GPU
  pack through `GENESIS_GPU_SCHEDULE_PACK=none|primary|matrix`; manual full runs
  use the closed `gpu_pack` input. `none` is a typed `unsupported-profile`
  nonclaim. A requested pack is release-relevant and must resolve every selected
  lane to at least one online runner containing the complete required label set.
- `gpu_runner_preflight` always runs on `ubuntu-24.04` before any self-hosted job.
  It queries repository runner inventory only for a requested pack, writes a
  `genesis/ci-runner-preflight-v0.1` artifact, and exports per-lane dispatch
  booleans. No self-hosted job has any other dispatch path. Missing labels,
  offline runners, or unavailable inventory for a requested pack are typed
  `infrastructure-failure`; an unrequested lane is `unsupported-profile` and can
  never be relabeled as release qualification. The preflight terminates within
  300 seconds.
- `.github/workflows/ci-watchdog.yml` is a separately scheduled observer with a
  unique per-run concurrency group and read-only Actions access. It evaluates
  `.github/workflows/ci.yml` history rather than its own status and retains a
  `genesis/ci-liveness-watchdog-v0.1` disposition. It classifies push-fast,
  separately dispatched standard, and full runs independently. It rejects a
  latest-main push or exact-head standard run without a successful terminal
  disposition after 7,200 seconds, a failed or cancelled latest exact-head
  standard attempt, wrong-head or superseded-only standard evidence, any scheduled
  or explicitly named full dispatch still running after 3,600 seconds, successful
  full evidence older than 172,800 seconds, an unsuccessful latest full run, a
  missing daily schedule after 93,600
  seconds, and a successful full run whose revision is absent from canonical
  `main` history. Watchdog output is observational and always has
  `releaseQualified = false`; it reports whether successful standard and full
  evidence form an exact-head pair without requiring a full run after every push.
- The full-profile dependency graph also enforces the 3,600-second ceiling rather
  than relying on observation alone. Named-target preparation is limited to 20
  minutes in parallel with each 55-minute release-evidence worker, followed by a
  5-minute aggregate;
  the full test and local
  workspace lanes are limited to 55 minutes with a 5-minute aggregate; hosted
  and selected self-hosted GPU paths use a 5-minute preflight, 50-minute lane,
  and 5-minute aggregate. Independent branches such as WebXR have shorter
  limits. A timeout is a terminal failure, never release evidence. Queue latency
  remains visible to the watchdog and cannot be hidden by a job-level timeout.
- `docs/program/incidents/CI_LIVENESS_2026-07-18_2026-08-04.json` is the
  append-only disposition for the first observed blackout: the 31-run sequence
  after run `29664738972` contains exactly 15 failures and 16 cancellations,
  binds every run identity and terminal timestamp, records the zero-runner and
  shared-pending-group observations, and retains the remaining full-profile
  nonclaim. Its canonical record digest is pinned by the control-plane policy;
  changing, dropping, reordering, or relabeling a historical run fails the
  execution-profile gate.
- `docs/program/incidents/CI_RELEASE_FULL_CHRONOLOGY_2026-08-05_2026-08-08.json`
  is the append-only follow-up for the release-profile redesign. It retains every
  scheduled or explicitly dispatched `full` run from the first measured redesign
  counterexample through the first corrected exact-`main` success, including
  intermediate successes followed by regressions. The control-plane policy pins
  its closed selection, boundaries, outcome counts, and canonical record digest.
  The independent watchdog verifies all retained ledgers before evaluating live
  history; neither ledger qualifies a product release or turns an unsupported
  target disposition into a platform claim.
- `docs/program/incidents/CI_STANDARD_CHRONOLOGY_2026-08-08.json` retains the
  bounded standard-profile failure sequence that exposed the push-fast
  substitution bug. Its closed run selection, terminal outcomes, and canonical
  digest are policy-pinned; the historical disposition remains non-authorizing
  even after the corrected watchdog observes later exact-head success.
- Standard pull-request CI runs the changed-impact planner with `--dry-run`
  because the same job executes generated-authority checks, lint, and the full
  test surface directly. This prevents a second disposable-worktree compilation
  from being mistaken for an iteration regression. Scheduled/manual `full` runs
  and post-merge `fast` runs retain measured changed-loop execution.
- `fast`: runs `scripts/test_changed_fast.sh` (default local/CI fast path)
- `standard|full`:
  - installs nextest
  - uses deterministic shard execution when `GENESIS_TEST_SHARDS_TOTAL > 1`
  - otherwise runs full workspace tests with nextest (`--cargo-profile selfhost-strict`)
  - preserves existing strict/smoke/golden gates as separate steps
  - CI runs `scripts/update_ai_stress_suite_report.sh` to enforce deterministic
    high-throughput stress coverage for tasks + bridge + gpu/compute + replay
    integrity and retain the uploadable E0 report/history set. Local validation
    uses the read-only `scripts/check_ai_stress_suite.sh` surface.
  - CI runs `scripts/update_backend_starter_workflows_report.sh` and
    `scripts/update_domain_starter_registry_bootstrap_report.sh` to retain
    backend scaffold/bridge replay and signed starter publish/pull/install
    evidence. Their `check_*` surfaces execute the same workflows entirely
    against private report destinations.
  - runs `scripts/check_task_concurrency_stress.sh` and
    `scripts/check_host_bridge_fault_injection.sh` as read-only real-test gates; retain their
    E0 report/history sets only with `scripts/update_task_concurrency_stress_report.sh` and
    `scripts/update_host_bridge_fault_injection_report.sh`.
    The spawn-per-op and persistent hard-cancellation loops are marked `stress-gate` and execute
    only through this host-bridge gate, never through default workspace tests.
  - release-full enables the composite `--kernel-tail-stress` mode of `scripts/test_perf_gates.sh`:
    it first runs `scripts/check_kernel_tcb_contract.sh` under the structural check's unchanged
    local-fast telemetry envelope, then the ignored
    stress case executes ten million bounded tail iterations in both treewalk and compiled modes.
    It requires exactly `90000009` steps and maximum evaluator call depth `3` per mode, with a
    `300000ms` wall budget and `536870912`-byte cold-cache growth budget. Default workspace tests
    retain the exact small-loop and one-step-short controls.
  - `scripts/check_gc_agent_task_cards.sh` owns the ignored parallel Rust/Python agent-plan
    selector parity stress case; the default suite retains only single-invocation contract tests.
  - `persistent_sharing_stress` retains 4,097 versions of 4,096-element vectors and maps and exercises
    equally sized ordered-map updates, retained strings, package dependency graphs, effect logs,
    and workspace snapshots. The isolated lane enforces a 120,000 ms wall ceiling and 16 MiB
    canonical-size ceilings; the default suite retains an eight-element non-vacuity control.
  - runs `scripts/check_agent_reference_workflows.sh` as the scored
    agent-capability gauntlet (`genesis/agent-capability-gauntlet-v0.1`) with
    required domain thresholds for service, network/process, raw-network,
    inbound-server, durable-data, package-publish/sync, graphics, gpu/compute, filesystem,
    process-lifecycle, plugin-runtime, and time-control workflows.
  - runs `scripts/check_agent_scenario_perf.sh` for aggregated end-to-end scenario latency SLOs
    (median + p95 + regression policy) derived from gauntlet workflow durations.
  - runs `scripts/check_agent_generative_workloads.sh` for mutation-based workload validation
    beyond the fixed reference workflow list.
  - full release-profile workflows always require the hosted deterministic GPU
    lane. When the independently selected `primary` or `matrix` hardware pack is
    claimed, exact-label preflight additionally requires the corresponding
    self-hosted lanes and retained lane-contract parity via
    `scripts/update_gpu_device_conformance_lane_parity_report.sh`. Core full runs
    with `gpu_pack=none` retain `unsupported-profile` and make no hardware-pack
    claim; local checks remain read-only.
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
  `GENESIS_HEALTH_SHARDS`) and profile runtime history controls
- release-full strict loop wall-time budget default (`GENESIS_HEALTH_RELEASE_FULL_BUDGET_MS`)
  and profile runtime history controls
- strict/full measured runtime gate wiring:
  - strict-golden profile runtime report + p95 budget helper
  - wasm cross-host runtime report + p95 budget helper
  - full-cross-host aggregate runtime budget gate command in CI
  - runtime-workload-bench report/history + runtime p95 budget gate command in CI

## Determinism

- Shard selection is deterministic from:
  - shard total/index
  - seed (`GENESIS_TEST_SHARD_SEED` or `GITHUB_SHA` in CI)
  - stable sorted crate list
- Runner selection is explicit in reports (`runner = cargo|nextest`).
