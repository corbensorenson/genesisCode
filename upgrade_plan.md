# GenesisCode Upgrade Plan - Open Red-Team Backlog (Self-Hosted + AI-First v1)

Last updated: 2026-02-20

This file contains only unresolved roadblocks from a fresh full-project red-team pass.

Open checklist items: 0

## P0 - Self-Host Cutover Blockers

- [x] P0.1 Remove Rust frontend/engine values from production CLI parse surface (fail-closed, not runtime-rejected).
  Evidence:
  - `crates/gc_cli_driver/src/cli_args.rs:71` keeps `CoreformFrontendArg::{Rust,Selfhost}` and accepts `"rust"` in parser.
  - `crates/gc_cli_driver/src/cli_args.rs:430` keeps `FmtEngine::{Rust,Selfhost}` and accepts `"rust"` in parser.
  - `crates/gc_cli_driver/src/selfhost_frontend.rs:241` and `crates/gc_cli_driver/src/selfhost_frontend.rs:315` still carry Rust-frontend/engine branches.
  Acceptance:
  - `genesis` and `genesis_wasi` reject Rust engine/frontend at parse level.
  - Rust engine/frontend code paths compile only in explicit parity harness binaries.
  - Production help/spec output contains no Rust engine/frontend value paths.
  Status:
  - Production CLIs now fail-closed at parse time for `--engine rust` and `--coreform-frontend rust` (`crates/gc_cli_driver/src/cli_args.rs`), with parity harness binaries retaining Rust parsing support.
  - Guard/test coverage updated for parse-level rejection semantics:
    - `scripts/selfhost_default_profile_guard.sh`
    - `scripts/selfhost_release_profile_guard.sh`
    - `crates/gc_cli/tests/cli_coreform_frontend_profile.rs`
    - `crates/gc_wasi_cli/tests/cli_coreform_frontend_profile.rs`
    - `crates/gc_cli/tests/cli_selfhost_only.rs`
    - `crates/gc_wasi_cli/tests/cli_selfhost_only.rs`
  - Added release-profile parse-surface conformance guard:
    - `scripts/check_production_cli_parse_surface.sh`
    - CI wiring in `.github/workflows/ci.yml` (`Production CLI Parse Surface Guard`)
    - Verifies production binaries (`genesis`, `genesis_wasi`) reject Rust frontend at parse level while parity binaries retain compatibility mode.
  - Completed compile-time separation of Rust engine/frontend paths into parity-only build targets:
    - Added parity-only driver package `crates/gc_cli_driver_parity/Cargo.toml` (same source tree, parity feature profile).
    - `genesis_parity` and `genesis_wasi_parity` now link `gc_cli_driver_parity` (`crates/gc_cli/src/main_parity.rs`, `crates/gc_wasi_cli/src/main_parity.rs`).
    - Production driver `gc_cli_driver` now compiles `FmtEngine`/`CoreformFrontendArg` without Rust variants unless `feature = "parity-harness"` (`crates/gc_cli_driver/src/cli_args.rs`, `crates/gc_cli_driver/src/selfhost_frontend.rs`, `crates/gc_cli_driver/src/cmd_core.rs`, `crates/gc_cli_driver/src/cmd_security_ops.rs`, `crates/gc_cli_driver/src/lib.rs`).
    - Verified with:
      - `bash scripts/check_production_cli_parse_surface.sh`
      - `cargo test -p gc_cli --test cli_coreform_frontend_profile --quiet`
      - `cargo test -p gc_wasi_cli --test cli_coreform_frontend_profile --quiet`
      - `cargo test -p gc_cli --test cli_fmt_engine --quiet`
      - `cargo test -p gc_wasi_cli --test cli_fmt_engine --quiet`

- [x] P0.2 Make selfhost-boundary enforcement strict and fail-closed in CI.
  Evidence:
  - `.github/workflows/ci.yml:85` runs `scripts/check_selfhost_boundary.sh` without `--strict`.
  - `scripts/check_selfhost_boundary.sh:20` allowlists all of `crates/gc_cli_driver/src/*`.
  - `scripts/check_selfhost_boundary.sh:140` exits success when no diff base is detected.
  Acceptance:
  - CI runs strict mode (`--strict`) on all production Rust files.
  - No-success-on-skip behavior when base detection fails.
  - Allowlist reduced to concrete files/functions instead of broad directory patterns.
  Status:
  - CI now runs `scripts/check_selfhost_boundary.sh --strict` (`.github/workflows/ci.yml`).
  - Diff mode now escalates to strict mode when no merge base is available (`scripts/check_selfhost_boundary.sh`).
  - `gc_cli_driver` allowlist is now explicit (`cmd_*.rs`, `selfhost_bridge.rs`, `pkg_self_opt.rs`, `kernel_exec.rs`).

- [x] P0.3 Replace implicit filename-order Prelude assembly with explicit manifest ordering and dependency validation.
  Evidence:
  - `scripts/assemble_prelude.sh:13` uses `find ... | sort` for module order.
  - `prelude/modules/README.md:17` points to the same script (no explicit module dependency manifest).
  Acceptance:
  - Add `prelude/modules/manifest` with explicit load order and dependency edges.
  - Assembly fails on missing/extra modules or dependency cycles.
  - Artifact freshness checks verify manifest hash in addition to concatenated output bytes.
  Status:
  - Added `prelude/modules/manifest.toml` with explicit ordered module list + dependency edges.
  - `scripts/assemble_prelude.sh` now reads and validates manifest ordering/dependencies, rejects unlisted or missing module files, and assembles deterministically from manifest order.
  - Added manifest-hash artifact `prelude/prelude.manifest.sha256`; `crates/gc_prelude/tests/prelude_modularization.rs` now verifies hash freshness plus assembly parity.

- [x] P0.4 Remove unbounded trusted bootstrap resource bypass.
  Evidence:
  - `crates/gc_prelude/src/selfhost_coreform_v1.rs:314` sets `ctx.step_limit = None` inside trusted bootstrap.
  - `crates/gc_prelude/src/selfhost_coreform_v1.rs:315` resets memory limits to defaults in the same path.
  Acceptance:
  - Trusted bootstrap uses bounded, profile-controlled limits.
  - Limits and observed counters are emitted into bootstrap evidence.
  - OOM/step exhaustion during bootstrap is surfaced as deterministic sealed errors, not bypassed.
  Status:
  - Replaced unbounded bootstrap override with profile-scoped bounded budgets in `trusted_bootstrap_budget()` (`crates/gc_prelude/src/selfhost_coreform_v1.rs`).
  - Added observed-kernel counter plumbing (`EvalCtx::observed_counters`, `EvalObservedCounters`, `MemObservedCounters`) and exported it via `gc_kernel` public API (`crates/gc_kernel/src/eval.rs`, `crates/gc_kernel/src/lib.rs`).
  - Bootstrap now emits deterministic evidence binding `core/selfhost::bootstrap-evidence` including limits, observed counters, stage identity, and deterministic failure context (`crates/gc_prelude/src/selfhost_coreform_v1.rs`).
  - Added bounded-budget regression test coverage in `trusted_bootstrap_budget_is_bounded_and_profile_controlled` (`crates/gc_prelude/src/selfhost_coreform_v1.rs`).

- [x] P0.5 Ship a first-party registry server runtime and CLI command.
  Evidence:
  - `docs/REGISTRY_PROTOCOL_MINIMAL_v0.1.md:25`-`56` defines server endpoints (`/v1/ping`, `/v1/store/*`, `/v1/refs/*`).
  - `crates/gc_cli_driver/src/cli_args.rs:597` `SyncCmd` exposes only `Pull` and `Push`; no serve command.
  - No non-test `impl InProcRegistry` exists under `crates/` (only test implementation in `crates/gc_registry/tests/chunk_upload.rs:56`).
  Acceptance:
  - Add `genesis registry serve` (native) with protocol-compliant endpoints.
  - Add end-to-end publish/install integration tests against that server.
  - Keep policy gating for `refs/set` enforced server-side.
  Status:
  - Added first-party HTTP registry server runtime in `gc_registry`:
    - `crates/gc_registry/src/server.rs`
    - exported via `crates/gc_registry/src/lib.rs` (`spawn_http_file_registry_server`, config/handle types).
  - Added native CLI command surface:
    - `genesis registry serve` in `crates/gc_cli_driver/src/cli_args.rs`
    - dispatch implementation in `crates/gc_cli_driver/src/cmd_registry.rs`
    - command is explicitly denied on WASI flavor with deterministic error contract.
  - Added end-to-end tests against the HTTP server runtime:
    - `crates/gc_cli/tests/cli_registry_http.rs` (policy-gated `pkg publish` + `sync push/pull` install-style roundtrip over HTTP)
    - `crates/gc_cli/tests/cli_registry_serve.rs` (CLI smoke for `registry serve` lifecycle).
  - Server-side `refs/set` policy gating remains enforced by existing file-registry policy checks (`file_gate_refs_set`) through the shared backend path in `gc_registry`.

- [x] P0.6 Close WASI remote sync gap for HTTP registries.
  Evidence:
  - `crates/gc_registry/src/lib.rs:1024` returns `wasi_http_unsupported(...)`.
  - `crates/gc_registry/src/lib.rs:1026` explicitly states HTTP(S) remotes are unsupported in WASI builds.
  Acceptance:
  - Provide a WASI-safe HTTP transport path (host bridge/proxy/capability adapter) for registry operations.
  - `genesis_wasi` can perform sync pull/push with HTTP remotes under policy constraints.
  - Add deterministic replay tests for WASI remote flows.
  Status:
  - Added WASI-safe HTTP bridge adapter in `gc_registry`:
    - `GENESIS_WASI_HTTP_BRIDGE_ROOT` maps HTTP(S) remotes to a file-backed registry root (`v1` auto-detected when present).
    - `RegistryClient::new_with_auth` now routes HTTP(S) remotes through `RegistryKind::File` when bridge is configured, avoiding direct WASI HTTP dependency.
    - Bridge availability exported via `gc_registry::wasi_http_bridge_configured()`.
  - Updated WASI remote profile gating to allow HTTP(S) only when bridge adapter is configured (`crates/gc_effects/src/runner_remote_ops.rs`), while preserving policy checks (`allow_http`, `remote_allow`, `wasi_network_profile`).
  - Added WASI CLI integration coverage:
    - `crates/gc_wasi_cli/tests/cli_sync.rs::wasi_sync_local_profile_http_bridge_roundtrip_and_log_determinism`
    - Verifies push/pull over HTTP remote string under `wasi_network_profile=local` with bridge enabled.
    - Verifies deterministic log equivalence across repeated equivalent pull flows.

## P1 - Performance and Reliability

- [x] P1.1 Remove per-command shell/process cold starts from `test_changed_fast` execution path.
  Evidence:
  - `scripts/test_changed_fast.sh:316` iterates commands serially.
  - `scripts/test_changed_fast.sh:318` executes each command through `bash -lc`, forcing repeated shell startup.
  Acceptance:
  - Replace per-command shell spawning with direct process execution or batched `nextest` invocations.
  - Keep equivalent test selection semantics and deterministic reporting.
  - Demonstrate lower wall-clock on unchanged/low-diff loops with the same coverage set.
  Status:
  - `scripts/test_changed_fast.sh` now executes selected commands in-process (`eval`) instead of spawning `bash -lc` per command.

- [x] P1.2 Stop rebuilding full selfhost artifact on every fast-loop freshness check.
  Evidence:
  - `scripts/check_selfhost_artifact_fresh.sh:25` always runs `genesis selfhost-artifact` to rebuild.
  - `scripts/test_fast.sh:20` runs freshness check in the local fast path.
  Acceptance:
  - Introduce source-manifest/hash staleness detection before rebuild.
  - Keep byte-for-byte artifact correctness guarantees.
  - Preserve deterministic failure behavior when committed artifact is stale.
  Status:
  - `scripts/check_selfhost_artifact_fresh.sh` now computes source hash from `selfhost/toolchain_manifest.gc` + referenced modules and compares against committed metadata before deciding to rebuild.
  - Added metadata artifact `selfhost/toolchain.freshness.json` and updater `scripts/update_selfhost_freshness_metadata.sh`.
  - Fast path skips rebuild when source+artifact hashes match metadata; slow path retains byte-for-byte rebuild comparison and stale failure behavior.

- [x] P1.3 Bring integration test sprawl under explicit budgets and modularization targets.
  Evidence:
  - `crates/gc_cli/tests/cli_stage1_pipeline.rs` is 1622 lines.
  - `crates/gc_cli/tests/cli_selfhost_only.rs` is 1603 lines.
  - `policies/source_size_budget.toml:15` excludes `/tests/` from size budgets.
  Acceptance:
  - Add dedicated test-file size budgets with debt allowlists and burn-down plan.
  - Split oversized integration suites into narrower capability-focused modules.
  - Keep coverage parity while reducing per-file cognitive load for agent editing.
  Status:
  - Added dedicated test-size budget policy and guard:
    - `policies/test_size_budget.toml`
    - `scripts/check_test_size_budget.sh`
  - Wired test-size guard into CI and plan health checks:
    - `.github/workflows/ci.yml`
    - `scripts/check_upgrade_plan_health.sh`
  - Split oversized integration suites into focused modules:
    - `crates/gc_cli/tests/cli_stage1_pipeline.rs`
    - `crates/gc_cli/tests/cli_stage2_pipeline_part1.rs`
    - `crates/gc_cli/tests/cli_stage2_pipeline_part2.rs`
    - `crates/gc_cli/tests/cli_selfhost_only.rs`
    - `crates/gc_cli/tests/cli_selfhost_only_regressions.rs`
  - Preserved/updated behavioral coverage for selfhost-only and parity paths while eliminating stale expectations after parser-level fail-closed cutover.

- [x] P1.4 Expand panic-guard coverage to all production crates on user paths.
  Evidence:
  - `scripts/check_no_user_panics.sh:8`-`14` runs clippy checks on a subset of libraries only.
  - Current guard omits several production crates that still participate in user workflows.
  Acceptance:
  - Define and enforce full production crate list for panic/unwrap/expect lints.
  - Keep exemptions explicit, minimal, and documented.
  - Fail CI when coverage list drifts from workspace production membership.
  Status:
  - Replaced hardcoded panic guard subset with policy-driven, metadata-derived full production coverage in `scripts/check_no_user_panics.sh`.
  - Added explicit exemption policy file `policies/panic_guard.toml` (current minimal exemptions: `gc_runtime_bench`, parity-only binaries).
  - Guard now fails on unknown policy entries and on uncovered production workspace packages, enforcing coverage drift checks against workspace membership.

- [x] P1.5 Remove duplicate task-op entries from generic not-supported dispatcher path.
  Evidence:
  - `crates/gc_effects/src/runner.rs:209` routes `core/task::*` through dedicated task runtime first.
  - `crates/gc_effects/src/runner_capability_dispatch.rs:232`-`241` still lists the same task ops in the generic not-supported branch.
  Acceptance:
  - Single source of truth for capability op routing.
  - No duplicated task op declarations across dispatch layers.
  - Conformance guard ensures dispatch table consistency.
  Status:
  - Removed `core/task::*` duplicate entries from `runner_capability_dispatch` not-supported branch.
  - Host ABI conformance extraction now includes `runner_task.rs` so task ops are validated from their true dispatch source.

## P2 - AI-First Surface and Capability Architecture

- [x] P2.1 Unify graphics-compute architecture so compute is first-class (not split across overlapping namespaces).
  Evidence:
  - `prelude/modules/10_gfx.gc:30` and `prelude/modules/10_gfx.gc:77` expose compute pipeline/submit under `core/gfx/gpu::*`.
  - `prelude/modules/11_gpu_compute.gc:1`-`51` exposes parallel compute surface under `core/gpu/compute::*`.
  - `docs/spec/GFX_CAPS.md:18`-`50` documents compute primarily under `gfx/gpu::*` while host ABI also exposes `gpu/compute::*` (`docs/spec/HOST_ABI.md:124`-`136`).
  Acceptance:
  - Define one canonical compute surface and one compatibility layer.
  - Keep graphics-specific APIs separate from general GPU compute APIs.
  - Update host ABI + prelude wrappers + tests to one stable architecture.
  Status:
  - Canonicalized compute surface to `gpu/compute::*` via prelude wrapper routing:
    - `core/gfx/gpu::create-compute-pipeline` now forwards to `gpu/compute::create-compute-pipeline`
    - `core/gfx/gpu::submit-compute-graph` now forwards to `gpu/compute::submit`
    (`prelude/modules/10_gfx.gc`)
  - Added explicit compatibility alias normalization at host dispatch boundary:
    - `gfx/gpu::create-compute-pipeline -> gpu/compute::create-compute-pipeline`
    - `gfx/gpu::submit-compute-graph -> gpu/compute::submit`
    (`crates/gc_effects/src/runner_response_budget.rs`)
  - Removed overlapping compute ops from canonical host dispatch surface (`crates/gc_effects/src/runner_capability_dispatch.rs`).
  - Updated docs/spec and conformance artifacts:
    - `docs/spec/GFX_CAPS.md`
    - `docs/spec/HOST_ABI.md`
    - regenerated capability indices (`scripts/update_capability_indices.sh`).
  - Verified with `scripts/check_host_abi_conformance.sh`, `scripts/check_capability_indices.sh`, and `gc_effects` host backend tests.

- [x] P2.2 Publish machine-readable host ABI and prelude capability indices for agent planning.
  Evidence:
  - `docs/spec/HOST_ABI.md` is markdown-only.
  - `docs/spec` has no machine-readable HOST ABI artifact (JSON/TOML) equivalent to CLI schema docs.
  Acceptance:
  - Generate versioned `HOST_ABI` and prelude-capability indices in machine-readable form.
  - Add CI drift check from Rust dispatch + prelude wrappers -> generated ABI index.
  - Include canonical examples for payload/response terms per op family.
  Status:
  - Added machine-readable indices:
    - `docs/spec/HOST_ABI_INDEX_v0.1.json`
    - `docs/spec/PRELUDE_CAPABILITY_INDEX_v0.1.json`
  - Added generation and drift tooling:
    - `scripts/generate_capability_indices.py`
    - `scripts/update_capability_indices.sh`
    - `scripts/check_capability_indices.sh`
  - CI now enforces index freshness (`.github/workflows/ci.yml`), and health gate includes the same drift check.
  - Added docs with canonical family payload/response examples:
    - `docs/spec/HOST_ABI_INDEX_v0.1.md`
    - `docs/spec/PRELUDE_CAPABILITY_INDEX_v0.1.md`

## Recommended Execution Order

1. P0.1 -> P0.2 -> P0.3 -> P0.4
2. P0.5 -> P0.6
3. P1.1 -> P1.2 -> P1.3 -> P1.4 -> P1.5
4. P2.1 -> P2.2
