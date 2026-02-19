# GenesisCode Upgrade Plan — Red-Team Backlog (Self-Hosted v1)

Last updated: 2026-02-19

This plan contains only unfinished work discovered in a full red-team review.

Open checklist items: 0

## P0 — Self-Host Cutover Blockers

- [x] Remove runtime dependency on archived Rust semantic builders.
  - Risk: self-host cutover is not complete while runtime command planning still imports archived bootstrap code.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/lib.rs:28`
  - Acceptance: no `old_bootstrap/rust_semantics` import in active CLI/runtime paths; parity-only tooling moved fully out of production path.

- [x] Selfhost-route remaining non-routed command surfaces.
  - Risk: incomplete selfhost routing leaves production behavior partially Rust-owned.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/lib.rs:3172`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/lib.rs:3178`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/lib.rs:3184`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/lib.rs:3208`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/lib.rs:3232`
  - Acceptance: `keygen`, `sign`, `transparency-verify`, `verify`, and `policy/*` are selfhost-routed with strict artifact-only support and golden parity checks.

- [x] Remove Rust-engine compatibility mode from production profile.
  - Risk: `GENESIS_ALLOW_RUST_ENGINE` keeps a legacy semantics bypass path.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/lib.rs:1377`
  - Acceptance: production builds reject all Rust-engine runtime execution paths; compatibility mode moved to explicit offline parity harness only.

- [x] Collapse bootstrap modes to artifact-only for release runtime.
  - Risk: `artifact-preferred` / `embedded` modes preserve hidden fallback behavior.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_prelude/src/selfhost_coreform_v1.rs:255`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_prelude/src/selfhost_coreform_v1.rs:803`
  - Acceptance: release binaries support artifact-only bootstrap; non-artifact bootstrap is development-only and isolated from release codepaths.

- [x] Remove panic paths from selfhost bootstrap loader.
  - Risk: malformed or unavailable toolchain artifact/source can terminate process.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_prelude/src/selfhost_coreform_v1.rs:246`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_prelude/src/selfhost_coreform_v1.rs:551`
  - Acceptance: loader returns typed errors end-to-end with no `panic!/expect` in production path.

- [x] Define hard release gate for “Rust-to-old_bootstrap retirement”.
  - Risk: accidental runtime regressions when removing Rust bootstrap artifacts.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/docs/spec/BOOTSTRAP_OLD.md`
  - Acceptance: signed cutover checklist and CI gate proving zero runtime dependency on archived bootstrap modules.

## P1 — Modern Language Capability Surface

- [x] Implement real GPU capability backend for `gfx/gpu::*`.
  - Risk: modern workloads (games/simulation/compute) are blocked; current runtime returns not-supported.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_gpu_host.rs`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner.rs`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/lib.rs:966`
  - Acceptance: `gfx/gpu::*` ops execute on a real backend with deterministic request/response logging and replay semantics.

- [x] Implement window/input/audio capability backends.
  - Risk: language can describe interactive apps but cannot execute them through host capabilities.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_gfx_host.rs`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner.rs`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/lib.rs:893`
  - Acceptance: `gfx/window::*`, `gfx/input::*`, `gfx/audio::*` have policy-gated host implementations and conformance tests.

- [x] Implement editor capability backend contract surface.
  - Risk: editor/task integration exists at ABI/prelude level but is host-stubbed.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_editor_host.rs`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/tests/host_abi_surface.rs`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/lib.rs:976`
  - Acceptance: `editor/task::*`, `editor/watch::*`, `editor/plugin::*`, `editor/clipboard::*`, `editor/dialog::*` are implemented or split into a formal plugin ABI with clear host requirements.

- [x] Replace synthetic task runtime with true multithreaded scheduler.
  - Risk: current `core/task::*` semantics are queue/state simulation, not actual concurrent execution.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_task.rs`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/lib.rs:727`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/lib.rs:796`
  - Acceptance: task execution is actually parallel with deterministic scheduling logs, replay compatibility, cancellation guarantees, and policy budgets.

- [x] Add deterministic numeric support beyond `Int`.
  - Risk: graphics/physics/ML-style workloads are constrained by int-only primitive arithmetic.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_coreform/src/fixed_decimal.rs`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_coreform/src/lib.rs`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_kernel/src/eval.rs`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_kernel/src/tests.rs`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_coreform/src/tests.rs`
  - Acceptance: deterministic decimal/float (or fixed-point numeric tower) added with canonical printing/hashing rules and replay stability.

- [x] Add non-bootstrap WASI networking path for sync/store remotes.
  - Risk: WASI runtime cannot participate in remote sync/store workflows.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner.rs:877`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner.rs:3484`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_remote_ops.rs:16`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_registry/src/lib.rs:713`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/Cargo.toml:24`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_registry/Cargo.toml:18`
  - Acceptance: configurable WASI networking/profile support for `core/sync::*` and remote `core/store::*`, with strict policy gating.

## P2 — Security and Robustness Hardening

- [x] Remove production lock-poison panics in registry client paths.
  - Risk: mutex poisoning or lock failures can crash host runtime.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_registry/src/lib.rs:44`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_registry/src/lib.rs:158`
  - Acceptance: all lock operations are fallible and surfaced as deterministic `RegistryError` values.

- [x] Add authenticated registry transport policy (token/mTLS) and enforcement.
  - Risk: current remote interactions rely on URL allowlists without standardized auth transport policy.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_registry/src/lib.rs`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/policy.rs`
  - Acceptance: capability policy supports credential/material references; registry client/server enforce auth and emit auditable errors.

- [x] Add bounded quotas for `core/store::put` and cumulative run artifact growth.
  - Risk: unbounded local artifact writes can cause disk exhaustion.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner.rs:3319`
  - Acceptance: policy-enforced per-op and per-run byte quotas for put/log/store with deterministic `core/caps/resource-limit` errors.

- [x] Harden `io/fs::write` against TOCTOU symlink races.
  - Risk: pre-write symlink check is raceable between metadata check and write.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner.rs:6862`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner.rs:6871`
  - Acceptance: write path uses race-safe open/write strategy (`O_NOFOLLOW` / dirfd-relative) and negative tests for symlink swap attacks.

## P3 — Performance and Scale

- [x] Finish decomposition of remaining mega-files.
  - Risk: very large files slow review, increase defect rate, and block parallel development.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner.rs` (~7.8k), `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_remote_ops.rs` (~1.3k), `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_gc_ops.rs` (~0.5k), `/Users/corbensorenson/Documents/genesisCode/crates/gc_obligations/src/lib.rs` (~4.3k), `/Users/corbensorenson/Documents/genesisCode/crates/gc_obligations/src/obligation_gfx.rs` (~0.9k), `/Users/corbensorenson/Documents/genesisCode/crates/gc_obligations/src/obligation_lint.rs` (~0.5k), `/Users/corbensorenson/Documents/genesisCode/crates/gc_obligations/src/obligation_stage.rs` (~0.5k), `/Users/corbensorenson/Documents/genesisCode/crates/gc_opt/src/stage2_wasm.rs` (~6.3k), `/Users/corbensorenson/Documents/genesisCode/crates/gc_opt/src/stage2_wasm/collections_lowering.rs` (~1.1k), `/Users/corbensorenson/Documents/genesisCode/crates/gc_opt/src/stage2_wasm/strings_bytes_lowering.rs` (~1.1k), `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/lib.rs` (~3.9k), `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/selfhost_frontend.rs` (~0.4k), `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/cmd_security_ops.rs` (~0.6k)
  - Progress this pass: moved stage2 string/bytes lowering and collection lowering into dedicated modules; moved obligation lint/ai-style and stage validation blocks into dedicated modules; moved CLI selfhost frontend resolution and security command handlers into dedicated modules.
  - Acceptance: domain-split modules with unchanged behavior and focused ownership boundaries.

- [x] Route default CLI eval/run/test flows through compiled evaluator path.
  - Risk: hot paths still use tree-walking evaluator only, leaving significant throughput on the table.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/kernel_exec.rs`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/lib.rs`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_obligations/src/lib.rs`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli/tests/cli_compiled_backend.rs`
  - Acceptance: compiled path default for eligible workloads with strict parity, fallback guards, and measurable speedup targets.

- [x] Reduce local test iteration wall-clock below 10 minutes without coverage loss.
  - Risk: current integration matrix is large and duplicates native/WASI command suites; iteration remains slow.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/scripts/test_fast.sh`, `/Users/corbensorenson/Documents/genesisCode/scripts/test_shard_workspace.sh`, `/Users/corbensorenson/Documents/genesisCode/.config/nextest.toml`, `/Users/corbensorenson/Documents/genesisCode/.github/workflows/ci.yml`, `/Users/corbensorenson/Documents/genesisCode/docs/spec/TEST_EXECUTION_PROFILES_v0.1.md`
  - Acceptance: sharding + nextest + hot-path smoke defaults are active, with full-suite CI coverage preserved via `cargo nextest run --workspace --cargo-profile selfhost-strict` (WASI coverage remains in dedicated strict/smoke lanes).

- [x] Rework perf budget harnesses to remove compile/noise contamination.
  - Risk: current budget scripts mix compile time and runtime behavior (e.g., invoking cargo tests in timing windows).
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/scripts/check_hot_path_budgets.sh`
  - Acceptance: warm, repeatable runtime-only benchmarks with trend baselines and variance controls.

## P4 — AI-First Tooling Surface

- [x] Add stable semantic-edit protocol (AST node IDs + structural edit ops) for agentic coding loops.
  - Risk: AI agents rely on brittle text diffs without first-class semantic mutation API.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_patches/src/lib.rs`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_patches/tests/patch_matrix.rs`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/lib.rs`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli/tests/cli_semantic_edit.rs`, `/Users/corbensorenson/Documents/genesisCode/docs/spec/PATCH_SCHEMA.md`, `/Users/corbensorenson/Documents/genesisCode/docs/spec/CLI.md`, `/Users/corbensorenson/Documents/genesisCode/selfhost/patch_schema_v1.gc`
  - Acceptance: canonical semantic edit API/CLI + obligations integration + deterministic patch provenance.

- [x] Standardize machine-readable diagnostics schema across all commands.
  - Risk: mixed free-text errors impede robust autonomous remediation loops.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/diagnostics.rs`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/lib.rs`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli/tests/cli_json_and_exit.rs`, `/Users/corbensorenson/Documents/genesisCode/docs/spec/CLI.md`
  - Acceptance: every failure path emits versioned, typed diagnostic payloads with stable error codes and optional suggested fixes.

- [x] Add contract ABI/intent introspection index for autonomous planning.
  - Risk: agents cannot cheaply discover callable surfaces, effect rows, and policy requirements.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/pkg_abi.rs`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/lib.rs:755`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli/tests/cli_pkg_abi.rs`, `/Users/corbensorenson/Documents/genesisCode/docs/spec/GCPM_ABI_INDEX_v0.1.md`
  - Acceptance: CLI/API exports contract op tables, type/effect signatures, required capabilities, and obligation metadata via `genesis gcpm abi --pkg <package.toml>` (`kind = genesis/pkg-abi-v0.1`).

- [x] Add closed-loop self-optimization pipeline gated by translation validation.
  - Risk: optimizer progress and language evolution remain mostly manual.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/pkg_self_opt.rs`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/cmd_pkg.rs`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli/tests/cli_pkg_self_optimize.rs`, `/Users/corbensorenson/Documents/genesisCode/docs/spec/CLI.md`
  - Acceptance: automated propose/optimize/validate/apply loop that only promotes rewrites with deterministic proof artifacts and obligation success.
