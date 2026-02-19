# GenesisCode Upgrade Plan — Red-Team Backlog (Self-Hosted v1 Cutover)

Last updated: 2026-02-19

This file contains only unfinished work from a fresh full-project red-team pass.
Completed items were intentionally removed.

Open checklist items: 15

## P0 — Self-Host and Correctness Blockers

- [x] Remove legacy conflict resolver op from runtime + CLI builder paths.
  - Risk: legacy semantic op remains executable, preventing clean selfhost cutover.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner.rs:5017`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/program_builders/vcs.rs:270`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli/tests/cli_vcs_merge3_contract.rs:18`
  - Acceptance: no `core/vcs-low::resolve-conflict-legacy` in production runtime/builders/tests; replacement op path is fully covered by conformance tests.

- [x] Implement `:rename` patch op in runtime patch-apply pipeline.
  - Risk: semantic patch schema is incomplete; valid artifacts can fail at runtime.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner.rs:7102`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner.rs:4688`
  - Acceptance: `PatchOp::Rename` applies deterministically with negative/edge-case tests and replay parity.

- [x] Make selfhost parser/canonicalizer the default production frontend in WASM exports.
  - Risk: default public WASM APIs still run Rust frontend paths.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_wasm/src/lib.rs:121`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_wasm/src/lib.rs:416`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_wasm/src/lib.rs:745`
  - Acceptance: default `fmt/hash/eval` wasm exports route through selfhost frontend; Rust path remains parity-only (explicitly named/debug gated).

- [ ] Remove release-profile dependence on Rust-engine compatibility toggles.
  - Risk: fallback semantics remain available via environment/profile drift.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/selfhost_frontend.rs:12`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/selfhost_frontend.rs:32`, `/Users/corbensorenson/Documents/genesisCode/docs/spec/CLI.md:47`
  - Acceptance: release binaries reject Rust engine/frontend execution paths unconditionally; compat paths move to dedicated parity harness binaries.

- [x] Fix command-name wiring bugs in selfhost fallback guard calls.
  - Risk: diagnostics and guard attribution are incorrect (`pkg`/`gc`/`sync` swapped), weakening cutover signal quality.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/cmd_store.rs:166`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/cmd_sync.rs:240`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/cmd_pkg.rs:997`
  - Acceptance: each command reports its own name in guard failures; regression tests assert exact command tags.

- [x] Remove panic-prone JSON envelope conversion in CLI command paths.
  - Risk: `expect("json")` can terminate process in user-facing flows.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/cmd_store.rs:253`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/cmd_sync.rs:297`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/lib.rs:1665`
  - Acceptance: all envelope conversions are fallible (`Result`) with stable diagnostic codes; zero `expect("json")` outside tests.

- [x] Remove finite-step-limit `expect` panics in obligations.
  - Risk: policy/limit edge cases can hard-crash instead of returning typed errors.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_obligations/src/lib.rs:640`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_obligations/src/lib.rs:641`
  - Acceptance: step-limit resolution is total and returns `ObligationError` on all invalid states.

- [x] Eliminate compiled-evaluator fallback to term evaluator for legacy closures.
  - Risk: hot paths silently deopt and diverge from compiled backend assumptions.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_kernel/src/compiled.rs:846`
  - Acceptance: compiled execution handles closure application without term-eval fallback (or fallback is impossible by construction) with parity tests.

- [ ] Implement WASI remote transport for registry/sync workflows.
  - Risk: WASI builds cannot participate in distributed package/ref workflows.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_registry/src/lib.rs:279`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_registry/src/lib.rs:323`, `/Users/corbensorenson/Documents/genesisCode/docs/spec/WASI.md:43`
  - Acceptance: WASI supports policy-gated remote `store/refs/sync` transport (or ships an explicit alternative transport profile) with replayable logs.

## P1 — Capability Completeness (Modern Language Surface)

- [ ] Replace in-memory GPU simulator with real host backend.
  - Risk: `gfx/gpu::*` is currently simulated state, not hardware execution.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_gpu_host.rs:11`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/Cargo.toml:1`
  - Acceptance: production GPU backend (e.g., `wgpu`) with deterministic request/response logging and replay strategy.

- [ ] Split GPU compute from graphics in capability model.
  - Risk: compute workloads are forced into gfx namespace semantics.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_gpu_host.rs:42`
  - Acceptance: separate compute-oriented ops/contracts (queue, buffers, kernels, reductions) with graphics-independent policy controls.

- [ ] Replace in-memory window/input/audio simulation with real host integrations.
  - Risk: interactive app surface exists but does not bind to OS/window/audio subsystems.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_gfx_host.rs:9`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_gfx_host.rs:37`
  - Acceptance: native host backends for `gfx/window::*`, `gfx/input::*`, `gfx/audio::*` plus deterministic log/replay contract.

- [ ] Replace editor host stubs with real editor/plugin bridge.
  - Risk: current editor capabilities are synthetic (`dialog`, `watch`, `plugin`, task results).
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_editor_host.rs:107`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_editor_host.rs:135`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_editor_host.rs:209`
  - Acceptance: concrete editor bridge spec + host implementation + integration tests (no synthetic heartbeat/plugin echo behavior in production profile).

- [ ] Upgrade `core/task::*` from payload simulation to executable concurrent work units.
  - Risk: task system currently models queue/jobs structurally instead of running general closures/effect programs as tasks.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_task.rs:663`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_task.rs:706`
  - Acceptance: task spawn/await/cancel/status operates on real executable units with deterministic scheduling trace and replay checks.

- [ ] Add first-class deterministic parallel primitives for AI-first large program generation.
  - Risk: no high-level structured-concurrency surface for generated programs.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_task.rs:758`
  - Acceptance: contract-level `task-group`, bounded parallel map/reduce, channel primitives, and cancellation scopes with policy budgets.

## P2 — Performance and Scale

- [x] Remove O(n) task queue operations and repeated full scans in scheduler hot path.
  - Risk: queue promotion path scales poorly under high task counts.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_task.rs:636`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_task.rs:658`
  - Acceptance: amortized O(1) queue pop/push, cached running/queued counters, and benchmarked scheduler improvements.

- [x] Make worker-budget defaults deterministic across hosts.
  - Risk: `available_parallelism()` introduces machine-dependent behavior.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_task.rs:691`
  - Acceptance: deterministic policy-driven default worker count with explicit override and replay-stable behavior.

- [ ] Continue decomposition of mega Rust modules to improve maintainability and parallel work.
  - Risk: very large files increase defect rate and slow review/AI edits.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner.rs` (7826 lines), `/Users/corbensorenson/Documents/genesisCode/crates/gc_opt/src/stage2_wasm.rs` (6291 lines), `/Users/corbensorenson/Documents/genesisCode/crates/gc_obligations/src/lib.rs` (4317 lines), `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/lib.rs` (3908 lines)
  - Acceptance: split into domain modules with unchanged behavior, reduced file sizes, and ownership boundaries.

- [ ] Split monolithic selfhost source artifacts into modular GC units.
  - Risk: AI-first iteration is slowed by giant single-file toolchain blobs.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/selfhost/toolchain.gc` (661579 bytes), `/Users/corbensorenson/Documents/genesisCode/selfhost/cli_coreform_v1.gc` (164673 bytes)
  - Acceptance: modularized selfhost packages (import graph + stable interfaces) with equivalent artifact output and tests.

- [x] Reduce local test feedback loop to <5 minutes P95 without reducing coverage.
  - Risk: current loop still requires broad crate runs and expensive integration setup.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/scripts/test_changed_fast.sh:1`, `/Users/corbensorenson/Documents/genesisCode/scripts/warm_selfhost_cache.sh:1`, `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/test_changed_fast_history.jsonl:1`, `/Users/corbensorenson/Documents/genesisCode/.github/workflows/ci.yml:108`
  - Acceptance: changed-file-aware test selection, warmed selfhost artifact cache, and measured P95 local runtime under 5 minutes on baseline dev hardware.

- [x] Add dedicated runtime microbenchmarks and enforce regression budgets in CI.
  - Risk: perf regressions can slip in despite functional tests.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/.github/workflows/ci.yml:137`, `/Users/corbensorenson/Documents/genesisCode/scripts/check_perf_budgets.sh`
  - Acceptance: benchmark suite tracks evaluator, runner, patch apply, store/sync operations with thresholded CI gates and trend artifacts.

## P3 — Security and Robustness Hardening

- [x] Remove remaining user-path panics/unwraps/expects in production crates.
  - Risk: malformed input or edge conditions can still terminate process.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/lib.rs:1665`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_obligations/src/lib.rs:640`
  - Acceptance: non-test production code is panic-free on user input; failure paths return typed errors or sealed protocol errors.

- [x] Harden store integrity lifecycle (write/read/verify) with corruption detection tooling.
  - Risk: silent corruption can poison replay/publish/install flows.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/store.rs:18`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner.rs:3319`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/cmd_store.rs:189`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli/tests/cli_store.rs:253`
  - Acceptance: periodic integrity scan command, per-artifact verification metadata, and deterministic failure codes for corrupted blobs.

- [ ] Enforce policy parity for publish/install/refs across native and WASI profiles.
  - Risk: cross-host behavior can diverge for obligation/auth/signature gates.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/docs/spec/WASI.md:22`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_registry/src/lib.rs:279`
  - Acceptance: policy-required checks (obligations/evidence/signatures) are identical across supported host profiles or explicitly profiled with hard fail.

- [x] Add end-to-end transport auth profile tests for registry operations.
  - Risk: auth policy may drift without concrete integration coverage.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_registry/src/lib.rs:65`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_registry/src/lib.rs:261`
  - Acceptance: integration tests for token/basic/mTLS profile wiring, denial modes, and audit logs.

## P4 — Self-Host Cutover Execution

- [ ] Define and run a single “Rust bootstrap retirement” gate.
  - Risk: partial cutover leaves hidden runtime dependencies.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/old_bootstrap`, `/Users/corbensorenson/Documents/genesisCode/docs/spec/BOOTSTRAP_OLD.md`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/selfhost_frontend.rs:39`
  - Acceptance: CI gate proves production runtime executes only selfhost toolchain paths; rust bootstrap moved to archival-only tooling.

- [ ] Remove production fallback routing after selfhost parity proof.
  - Risk: dead/legacy branches accumulate and create future correctness/security risk.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/selfhost_frontend.rs:304`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli/tests/cli_selfhost_only.rs:863`
  - Acceptance: fallback branches removed in production profile, parity harness retained separately, and docs/spec updated to single execution model.

- [ ] Ship selfhost-native project manager (`gcpm`) as the canonical workflow path.
  - Risk: mixed legacy command surfaces fragment package/workspace workflows.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/docs/spec/GCPM_CLI_CONTRACT_v0.1.md`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli/tests/cli_gcpm_selfhost_acceptance.rs:108`
  - Acceptance: `gcpm` covers init/add/lock/install/update/export/import/publish/verify workflows in selfhost-only mode with stable machine-readable outputs.

- [ ] Add selfhost-only GPU/parallel reference programs and obligations.
  - Risk: selfhost claim is incomplete without proving advanced workloads run through GC stack.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_gpu_host.rs:42`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_task.rs:648`
  - Acceptance: reference suites (compute + rendering + parallel tasks) pass under selfhost-only execution with replay/evidence artifacts.
