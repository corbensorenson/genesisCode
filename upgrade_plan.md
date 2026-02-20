# GenesisCode Upgrade Plan - Open Red-Team Backlog (Self-Hosted + AI-First v1)

Last updated: 2026-02-20

This file contains only unresolved roadblocks from a fresh full-project red-team pass.

Open checklist items: 13

## P0 - Self-Host Cutover Blockers

- [ ] P0.1 Remove Rust frontend/engine values from production CLI parse surface (fail-closed, not runtime-rejected).
  Evidence:
  - `crates/gc_cli_driver/src/cli_args.rs:71` keeps `CoreformFrontendArg::{Rust,Selfhost}` and accepts `"rust"` in parser.
  - `crates/gc_cli_driver/src/cli_args.rs:430` keeps `FmtEngine::{Rust,Selfhost}` and accepts `"rust"` in parser.
  - `crates/gc_cli_driver/src/selfhost_frontend.rs:241` and `crates/gc_cli_driver/src/selfhost_frontend.rs:315` still carry Rust-frontend/engine branches.
  Acceptance:
  - `genesis` and `genesis_wasi` reject Rust engine/frontend at parse level.
  - Rust engine/frontend code paths compile only in explicit parity harness binaries.
  - Production help/spec output contains no Rust engine/frontend value paths.

- [ ] P0.2 Make selfhost-boundary enforcement strict and fail-closed in CI.
  Evidence:
  - `.github/workflows/ci.yml:85` runs `scripts/check_selfhost_boundary.sh` without `--strict`.
  - `scripts/check_selfhost_boundary.sh:20` allowlists all of `crates/gc_cli_driver/src/*`.
  - `scripts/check_selfhost_boundary.sh:140` exits success when no diff base is detected.
  Acceptance:
  - CI runs strict mode (`--strict`) on all production Rust files.
  - No-success-on-skip behavior when base detection fails.
  - Allowlist reduced to concrete files/functions instead of broad directory patterns.

- [ ] P0.3 Replace implicit filename-order Prelude assembly with explicit manifest ordering and dependency validation.
  Evidence:
  - `scripts/assemble_prelude.sh:13` uses `find ... | sort` for module order.
  - `prelude/modules/README.md:17` points to the same script (no explicit module dependency manifest).
  Acceptance:
  - Add `prelude/modules/manifest` with explicit load order and dependency edges.
  - Assembly fails on missing/extra modules or dependency cycles.
  - Artifact freshness checks verify manifest hash in addition to concatenated output bytes.

- [ ] P0.4 Remove unbounded trusted bootstrap resource bypass.
  Evidence:
  - `crates/gc_prelude/src/selfhost_coreform_v1.rs:314` sets `ctx.step_limit = None` inside trusted bootstrap.
  - `crates/gc_prelude/src/selfhost_coreform_v1.rs:315` resets memory limits to defaults in the same path.
  Acceptance:
  - Trusted bootstrap uses bounded, profile-controlled limits.
  - Limits and observed counters are emitted into bootstrap evidence.
  - OOM/step exhaustion during bootstrap is surfaced as deterministic sealed errors, not bypassed.

- [ ] P0.5 Ship a first-party registry server runtime and CLI command.
  Evidence:
  - `docs/REGISTRY_PROTOCOL_MINIMAL_v0.1.md:25`-`56` defines server endpoints (`/v1/ping`, `/v1/store/*`, `/v1/refs/*`).
  - `crates/gc_cli_driver/src/cli_args.rs:597` `SyncCmd` exposes only `Pull` and `Push`; no serve command.
  - No non-test `impl InProcRegistry` exists under `crates/` (only test implementation in `crates/gc_registry/tests/chunk_upload.rs:56`).
  Acceptance:
  - Add `genesis registry serve` (native) with protocol-compliant endpoints.
  - Add end-to-end publish/install integration tests against that server.
  - Keep policy gating for `refs/set` enforced server-side.

- [ ] P0.6 Close WASI remote sync gap for HTTP registries.
  Evidence:
  - `crates/gc_registry/src/lib.rs:1024` returns `wasi_http_unsupported(...)`.
  - `crates/gc_registry/src/lib.rs:1026` explicitly states HTTP(S) remotes are unsupported in WASI builds.
  Acceptance:
  - Provide a WASI-safe HTTP transport path (host bridge/proxy/capability adapter) for registry operations.
  - `genesis_wasi` can perform sync pull/push with HTTP remotes under policy constraints.
  - Add deterministic replay tests for WASI remote flows.

## P1 - Performance and Reliability

- [ ] P1.1 Remove per-command shell/process cold starts from `test_changed_fast` execution path.
  Evidence:
  - `scripts/test_changed_fast.sh:316` iterates commands serially.
  - `scripts/test_changed_fast.sh:318` executes each command through `bash -lc`, forcing repeated shell startup.
  Acceptance:
  - Replace per-command shell spawning with direct process execution or batched `nextest` invocations.
  - Keep equivalent test selection semantics and deterministic reporting.
  - Demonstrate lower wall-clock on unchanged/low-diff loops with the same coverage set.

- [ ] P1.2 Stop rebuilding full selfhost artifact on every fast-loop freshness check.
  Evidence:
  - `scripts/check_selfhost_artifact_fresh.sh:25` always runs `genesis selfhost-artifact` to rebuild.
  - `scripts/test_fast.sh:20` runs freshness check in the local fast path.
  Acceptance:
  - Introduce source-manifest/hash staleness detection before rebuild.
  - Keep byte-for-byte artifact correctness guarantees.
  - Preserve deterministic failure behavior when committed artifact is stale.

- [ ] P1.3 Bring integration test sprawl under explicit budgets and modularization targets.
  Evidence:
  - `crates/gc_cli/tests/cli_stage1_pipeline.rs` is 1622 lines.
  - `crates/gc_cli/tests/cli_selfhost_only.rs` is 1603 lines.
  - `policies/source_size_budget.toml:15` excludes `/tests/` from size budgets.
  Acceptance:
  - Add dedicated test-file size budgets with debt allowlists and burn-down plan.
  - Split oversized integration suites into narrower capability-focused modules.
  - Keep coverage parity while reducing per-file cognitive load for agent editing.

- [ ] P1.4 Expand panic-guard coverage to all production crates on user paths.
  Evidence:
  - `scripts/check_no_user_panics.sh:8`-`14` runs clippy checks on a subset of libraries only.
  - Current guard omits several production crates that still participate in user workflows.
  Acceptance:
  - Define and enforce full production crate list for panic/unwrap/expect lints.
  - Keep exemptions explicit, minimal, and documented.
  - Fail CI when coverage list drifts from workspace production membership.

- [ ] P1.5 Remove duplicate task-op entries from generic not-supported dispatcher path.
  Evidence:
  - `crates/gc_effects/src/runner.rs:209` routes `core/task::*` through dedicated task runtime first.
  - `crates/gc_effects/src/runner_capability_dispatch.rs:232`-`241` still lists the same task ops in the generic not-supported branch.
  Acceptance:
  - Single source of truth for capability op routing.
  - No duplicated task op declarations across dispatch layers.
  - Conformance guard ensures dispatch table consistency.

## P2 - AI-First Surface and Capability Architecture

- [ ] P2.1 Unify graphics-compute architecture so compute is first-class (not split across overlapping namespaces).
  Evidence:
  - `prelude/modules/10_gfx.gc:30` and `prelude/modules/10_gfx.gc:77` expose compute pipeline/submit under `core/gfx/gpu::*`.
  - `prelude/modules/11_gpu_compute.gc:1`-`51` exposes parallel compute surface under `core/gpu/compute::*`.
  - `docs/spec/GFX_CAPS.md:18`-`50` documents compute primarily under `gfx/gpu::*` while host ABI also exposes `gpu/compute::*` (`docs/spec/HOST_ABI.md:124`-`136`).
  Acceptance:
  - Define one canonical compute surface and one compatibility layer.
  - Keep graphics-specific APIs separate from general GPU compute APIs.
  - Update host ABI + prelude wrappers + tests to one stable architecture.

- [ ] P2.2 Publish machine-readable host ABI and prelude capability indices for agent planning.
  Evidence:
  - `docs/spec/HOST_ABI.md` is markdown-only.
  - `docs/spec` has no machine-readable HOST ABI artifact (JSON/TOML) equivalent to CLI schema docs.
  Acceptance:
  - Generate versioned `HOST_ABI` and prelude-capability indices in machine-readable form.
  - Add CI drift check from Rust dispatch + prelude wrappers -> generated ABI index.
  - Include canonical examples for payload/response terms per op family.

## Recommended Execution Order

1. P0.1 -> P0.2 -> P0.3 -> P0.4
2. P0.5 -> P0.6
3. P1.1 -> P1.2 -> P1.3 -> P1.4 -> P1.5
4. P2.1 -> P2.2
