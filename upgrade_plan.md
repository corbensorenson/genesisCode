# GenesisCode Upgrade Plan - Red-Team Backlog (Self-Hosted v1)

Last updated: 2026-02-19

This file contains only unfinished work from a fresh red-team pass.
Completed work was intentionally removed.

Open checklist items: 1

## P0 - Self-Host Completion Blockers

- [x] Retire debug-only Rust engine compatibility from the main CLI binaries.
  - Risk: production behavior can still depend on Rust-only compatibility switches during local/dev execution, which weakens the self-host boundary.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/selfhost_frontend.rs:12`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/selfhost_frontend.rs:32`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/selfhost_frontend.rs:125`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/selfhost_frontend.rs:196`, `/Users/corbensorenson/Documents/genesisCode/docs/spec/CLI.md:42`
  - Completed (2026-02-19): added runtime profiles in `gc_cli_driver`; `genesis`/`genesis_wasi` always run production profile, while `genesis_parity`/`genesis_wasi_parity` provide explicit Rust parity surface.
  - Completed (2026-02-19): removed env-toggle compatibility from main CLI/runtime guards and migrated parity tests/scripts/docs to dedicated parity binaries.
  - Completed (2026-02-19): updated retirement gates (`check_rust_engine_compat`, `selfhost_default_profile_guard`, `selfhost_release_profile_guard`, `check_bootstrap_retirement_gate`) to enforce parity-binary-only Rust comparisons.
  - Acceptance: parity paths live only in dedicated parity harness binaries; `genesis` and `genesis_wasi` ship selfhost-only engine/frontend behavior without env toggles.

- [x] Remove tree-walk fallback as the default kernel execution contract.
  - Risk: compiled-vs-treewalk split introduces dual semantics and hidden deopt paths in core execution.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/kernel_exec.rs:6`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/kernel_exec.rs:37`, `/Users/corbensorenson/Documents/genesisCode/docs/spec/CLI.md:37`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/lib.rs:2300`
  - Acceptance: default runtime path is compiled-only in production; any fallback mode is explicit, non-default, and parity-only with separate telemetry.

- [x] Enforce pinned artifact bootstrap only (no implicit workspace/source fallbacks) for selfhost runtime commands.
  - Risk: fallback artifact discovery (`./.genesis/...` and workspace file fallback) can silently alter toolchain provenance.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/selfhost_frontend.rs:88`, `/Users/corbensorenson/Documents/genesisCode/docs/spec/CLI.md:32`, `/Users/corbensorenson/Documents/genesisCode/docs/spec/CLI.md:36`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/selfhost_frontend.rs:249`
  - Completed (2026-02-19): runtime engine paths (`fmt/eval/explain/run/replay/optimize/vcs hash`) now require `--selfhost-bootstrap artifact-only` plus explicit artifact identity (`--selfhost-artifact` or `GENESIS_SELFHOST_TOOLCHAIN_ARTIFACT`); implicit filesystem fallback is rejected.
  - Completed (2026-02-19): runtime JSON envelopes include `data.selfhost_artifact` (`null` for rust engine; `{path,hash,source:"explicit"}` for selfhost engine).
  - Acceptance: selfhost execution requires an explicit pinned artifact identity (hash + path or store hash), and reports it in every JSON envelope.

- [x] Expand Stage2 compiler support to cover the full v1 selfhost workload surface.
  - Risk: selfhost optimization/validation pipeline is incomplete while major expression and primitive classes remain unsupported.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_opt/src/stage2_wasm.rs:1679`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_opt/src/stage2_wasm.rs:3649`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_opt/src/stage2_wasm.rs:4159`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_opt/src/stage2_wasm.rs:6279`
  - Completed (2026-02-19): selfhost effect inference now tracks direct `core/task::*` and `core/editor/task::*` wrappers in `/Users/corbensorenson/Documents/genesisCode/selfhost/cli_reachability_v1.gc`, closing the strict-golden `pkg_gpu_parallel_obligations` parity blocker.
  - Completed (2026-02-19): strict-golden package sweep now provisions deterministic GPU bridge policy for `pkg_gpu_parallel_obligations` inside `/Users/corbensorenson/Documents/genesisCode/scripts/selfhost_strict_golden.sh`, eliminating runtime false negatives during obligation runs.
  - Completed (2026-02-19): `genesis selfhost-artifact --json` reports `stage2_supported_modules=11` and `stage2_validated_modules=11`, and `scripts/selfhost_strict_golden.sh` passes end-to-end.
  - Acceptance: Stage2 supports required pure CoreForm subset used by selfhost toolchain modules with obligation-backed parity coverage and no unsupported hot-path forms.

- [x] Tighten Stage2 gate semantics to fail closed for protected pipelines.
  - Risk: current gate behavior allows unsupported modules to pass through, reducing assurance of translation validation in release flows.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/docs/spec/CLI.md:74`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_wasm/src/lib.rs:1150`
  - Acceptance: protected commands (package acceptance/release/publish paths) require either Stage2 support+validation success or a policy-explicit override with signed waiver evidence.

- [x] Implement WASI host transport parity for bridge-backed capability domains.
  - Risk: editor/gfx/gpu capability families are non-functional in WASI due bridge execution being hard-disabled.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_host_bridge.rs:112`, `/Users/corbensorenson/Documents/genesisCode/docs/spec/CLI.md:29`, `/Users/corbensorenson/Documents/genesisCode/docs/spec/HOST_ABI.md:17`
  - Acceptance: WASI runtime supports bridge-backed host calls through a deterministic transport profile, including replay-compatible request/response handling.

## P1 - Capability Runtime Hardening

- [x] Replace env-var bridge payload transport with a framed protocol.
  - Risk: `GENESIS_HOST_BRIDGE_PAYLOAD` transport is fragile (size limits, escaping overhead, poor streaming behavior).
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_host_bridge.rs:70`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_host_bridge.rs:71`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_host_bridge.rs:75`
  - Acceptance: bridge uses deterministic framed stdin/stdout protocol (length-prefixed CoreForm/bytes frames), with strict parse/size/error contracts.

- [x] Enforce timeout and payload budgets for bridge-backed ops.
  - Risk: bridge-backed operations bypass timeout path used by `call_capability`, enabling potential hangs and unbounded payloads.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner.rs:185`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner.rs:210`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner.rs:851`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_host_bridge.rs:75`
  - Acceptance: `timeout_ms`, cancellation, and per-op byte limits are uniformly enforced for editor/gfx/gpu bridge calls and reflected in log decisions/errors.

- [x] Replace synthetic editor host behaviors with real bridge-backed integrations.
  - Risk: clipboard/watch/task operations still run local synthetic logic and are not authoritative for real editor automation workloads.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_editor_host.rs:81`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_editor_host.rs:373`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_editor_host.rs:419`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_editor_host.rs:444`
  - Acceptance: editor ops execute through stable host bridge contracts with deterministic log/replay semantics and no synthetic fallback in production profile.

- [x] Upgrade `core/task::*` from payload mini-DSL to executable effect programs/closures.
  - Risk: current task runtime executes a restricted map-step DSL rather than first-class Genesis computations.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_task.rs:326`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_task_exec.rs:12`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_task_exec.rs:72`, `/Users/corbensorenson/Documents/genesisCode/prelude/modules/00_core.gc:431`
  - Completed (2026-02-19): task executor now supports executable payloads via `:task/eval` (+ `:task/arg` / `:task/args`) with callable application and effect-program execution under the parent capability policy in `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_task_exec.rs`.
  - Completed (2026-02-19): worker/runtime plumbing passes policy through async task jobs so queued and worker-executed tasks share identical capability semantics in `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_task.rs`.
  - Completed (2026-02-19): prelude gained AI-oriented helpers `core/task::spawn-eval`, `core/task::spawn-eval1`, and `core/task::spawn-evaln` in `/Users/corbensorenson/Documents/genesisCode/prelude/modules/00_core.gc` and synchronized `/Users/corbensorenson/Documents/genesisCode/prelude/prelude.gc`.
  - Completed (2026-02-19): coverage added for callable/effect-program execution + replay and capability enforcement in `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/lib.rs`; strict parity gate `scripts/selfhost_strict_golden.sh` passes.
  - Acceptance: task spawn/await/cancel can run closure/effect-program payloads with deterministic scheduling traces and replay validation.

- [x] Split compute-first GPU APIs into a dedicated prelude module/package (not nested in gfx bundle).
  - Risk: compute workflow APIs are currently defined inside the gfx module, blurring graphics and general compute boundaries for AI-generated systems.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/prelude/modules/10_gfx.gc:80`, `/Users/corbensorenson/Documents/genesisCode/prelude/modules/10_gfx.gc:123`, `/Users/corbensorenson/Documents/genesisCode/docs/spec/HOST_ABI.md:89`, `/Users/corbensorenson/Documents/genesisCode/docs/spec/HOST_ABI.md:117`
  - Acceptance: compute contracts live in a dedicated module surface with independent docs, obligations, and capability policies from gfx scene/window paths.

## P2 - Spec and Boundary Consistency

- [x] Reconcile host operation namespaces across specs and runtime (`io/*` planned vs `gfx/*`/`gpu/compute::*` shipped).
  - Risk: boundary docs and ABI docs diverge, increasing implementation and policy drift risk.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/docs/spec/SELF_HOST_BOUNDARY.md:59`, `/Users/corbensorenson/Documents/genesisCode/docs/spec/SELF_HOST_BOUNDARY.md:60`, `/Users/corbensorenson/Documents/genesisCode/docs/spec/HOST_ABI.md:89`, `/Users/corbensorenson/Documents/genesisCode/docs/spec/HOST_ABI.md:117`
  - Acceptance: single normative namespace map with migration notes and CI checks that enforce cross-doc consistency.

- [x] Add normative bridge protocol spec and conformance tests for all host-integrated domains.
  - Risk: bridge behavior is implementation-defined today (spawn, payload channel, error mapping), making cross-host compatibility fragile.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_host_bridge.rs:12`, `/Users/corbensorenson/Documents/genesisCode/docs/spec/HOST_ABI.md:17`, `/Users/corbensorenson/Documents/genesisCode/docs/spec/WASM_HOST_BRIDGE.md:1`
  - Acceptance: new `docs/spec/HOST_BRIDGE_PROTOCOL.md` plus automated conformance tests for native and WASI profiles.

## P3 - Scalability, Maintainability, and Coverage

- [ ] Decompose Rust megafiles into domain modules with stable internal interfaces.
  - Risk: very large source files slow review, increase regression risk, and degrade AI editing quality.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner.rs` (~6152 LOC), `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_response_budget.rs` (~439 LOC), `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_vcs_pkg_helpers.rs` (~1144 LOC), `/Users/corbensorenson/Documents/genesisCode/crates/gc_opt/src/stage2_wasm.rs` (~6291 LOC), `/Users/corbensorenson/Documents/genesisCode/crates/gc_obligations/src/lib.rs` (~3766 LOC), `/Users/corbensorenson/Documents/genesisCode/crates/gc_obligations/src/frontend.rs` (~143 LOC), `/Users/corbensorenson/Documents/genesisCode/crates/gc_obligations/src/obligation_cache.rs` (~446 LOC), `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/lib.rs` (~3336 LOC), `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/cli_json.rs` (~63 LOC)
  - Completed (2026-02-19): extracted VCS + package-lock helper subsystem from `gc_effects::runner` into `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_vcs_pkg_helpers.rs`; this removed ~1140 LOC of cross-domain logic from `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner.rs` while keeping behavior unchanged.
  - Completed (2026-02-19): decomposition regression guard passed via `cargo test -p gc_effects --lib` (57 tests).
  - Completed (2026-02-19): extracted error/response serialization + artifact budget helpers from `gc_effects::runner` into `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_response_budget.rs`, shrinking runtime-loop file complexity without semantic drift.
  - Completed (2026-02-19): extracted selfhost frontend policy/config surface from `gc_obligations::lib` into `/Users/corbensorenson/Documents/genesisCode/crates/gc_obligations/src/frontend.rs` with public re-exports preserved.
  - Completed (2026-02-19): decomposition regression guard passed via `cargo test -p gc_obligations --lib` (24 tests).
  - Completed (2026-02-19): extracted obligation cache/acceptance persistence logic from `gc_obligations::lib` into `/Users/corbensorenson/Documents/genesisCode/crates/gc_obligations/src/obligation_cache.rs`, isolating deterministic cache policy from obligation orchestration.
  - Completed (2026-02-19): extracted CLI JSON canonicalization + envelope serialization helpers into `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/cli_json.rs`.
  - Completed (2026-02-19): decomposition regression guard passed via `cargo test -p gc_cli_driver` (13 tests).
  - Remaining for closure: split `gc_effects::runner` capability dispatch arms by domain (`store/refs/sync/pkg/vcs/gc`) into command modules; split `gc_opt::stage2_wasm` planning/lowering/execution pipelines into staged modules; split `gc_obligations::lib` remaining obligation execution orchestration into obligation-family modules; split `gc_cli_driver::lib` command handler bodies (`cmd_eval`/`cmd_run`/`cmd_replay`/selfhost dashboard/artifact) into focused modules.
  - Acceptance: each module is split by subsystem with focused tests and no behavior drift in existing conformance suites.

- [x] Decompose `selfhost/toolchain.gc` into smaller selfhost packages with explicit import graph.
  - Risk: the monolithic selfhost artifact source slows AI generation, review, and selective optimization.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/selfhost/toolchain.gc` (~6353 LOC), `/Users/corbensorenson/Documents/genesisCode/selfhost/toolchain_manifest.gc:1`
  - Completed (2026-02-19): selfhost source is module-split and assembled from manifest-defined module paths in `/Users/corbensorenson/Documents/genesisCode/selfhost/toolchain_manifest.gc`.
  - Completed (2026-02-19): deterministic artifact composition is gate-tested (`selfhost_artifact_is_byte_for_byte_deterministic_across_rebuilds`) in `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli/tests/cli_selfhost_artifact.rs`.
  - Acceptance: toolchain source is packageized (frontend, vcs/pkg/gcpm, optimizer, diagnostics) and assembled reproducibly through manifest-defined composition.

- [x] Raise WASI parity test coverage from smoke+strict lanes to regular shard coverage.
  - Risk: workspace test shard currently excludes `gc_wasi_cli`, allowing regressions to evade the primary CI test matrix.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/.github/workflows/ci.yml:122`, `/Users/corbensorenson/Documents/genesisCode/.github/workflows/ci.yml:124`
  - Acceptance: WASI CLI integration tests participate in standard CI sharding (or equivalent dedicated matrix with comparable depth and gating strength).

- [x] Add benchmark budgets for bridge-heavy and task-heavy workloads.
  - Risk: existing perf budgets may miss regressions in newly introduced bridge/task hot paths.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_host_bridge.rs:64`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_task.rs:549`, `/Users/corbensorenson/Documents/genesisCode/.github/workflows/ci.yml:146`
  - Acceptance: CI enforces explicit latency/throughput budgets for bridge dispatch, task scheduling, and replay overhead with trend artifacts.
