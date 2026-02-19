# GenesisCode Upgrade Plan — Red-Team + Optimization Backlog

Last updated: 2026-02-19

This plan contains active unfinished work derived from a targeted red-team review.

Open checklist items: 0

## High-Priority Security Hardening

- [x] Replace URL prefix allowlisting with origin/path-boundary matching for remotes.
  - Risk: `starts_with` checks can allow host confusion (`trusted.com.evil`).
  - Code refs: `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner.rs:9657`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner.rs:9675`.
  - Acceptance: parse allowlist into normalized URL components and require exact scheme/host/port plus constrained path prefix semantics.

- [x] Strengthen ref-policy gate: bind required obligations to required evidence kinds.
  - Risk: current gate checks obligation symbol presence + non-empty evidence, but not obligation->evidence-kind satisfaction.
  - Code refs: `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner.rs:7294`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_vcs/src/policy.rs:24`.
  - Acceptance: extend policy schema with required evidence kinds (and optionally obligation mappings), enforce at `refs::set` and publish paths.

- [x] Eliminate panic paths in production capability execution and sync worker aggregation.
  - Risk: `expect`-based lock/slot assumptions can terminate process under contention/poisoning.
  - Code refs: `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner.rs:7584`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner.rs:10068`.
  - Acceptance: replace `expect` with typed errors and convert to sealed ERROR at boundaries.

- [x] Add hard resource caps for remote artifact ingestion and bundle decode.
  - Risk: unbounded allocation/read paths permit memory DoS.
  - Code refs: `/Users/corbensorenson/Documents/genesisCode/crates/gc_registry/src/lib.rs:264`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_vcs/src/gpk.rs:176`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_vcs/src/gpk.rs:210`.
  - Acceptance: enforce max artifact bytes, max bundle entries, max per-entry bytes, and fail deterministically with explicit error codes.

- [x] Introduce cancellable timeouts for capability jobs.
  - Risk: current timeout returns early but underlying job continues, enabling runaway thread/work accumulation.
  - Code refs: `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner.rs:7567`.
  - Acceptance: cooperative cancellation token or bounded worker with drop-safe cancellation protocol and load tests.

## Performance + Scale

- [x] Split `gc_effects` runner into maintainable modules (sync/store/refs/pkg/vcs/gc/gpk/io/task).
  - Risk: single 11k-line file slows iteration, review, and regression isolation.
  - Code ref: `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner.rs`.
  - Acceptance: module boundaries with unchanged public behavior and parity tests preserved.
  - Completion (2026-02-19): extracted domain helpers into dedicated modules (`/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_timeout.rs`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_task.rs`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_io_ops.rs`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_store_ops.rs`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_refs_ops.rs`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_gpk_payload.rs`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_vcs_payload.rs`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_pkg_payload.rs`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_sync_payload.rs`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_gc_payload.rs`) and removed duplicated payload/parser blocks from `runner.rs`, with parity validated by targeted `gc_effects` sync/refs/timeout suites and workspace `cargo check`.

- [x] Split CLI driver monolith into command-domain modules with shared typed helpers.
  - Risk: single 8.8k-line file limits safe parallel development and increases merge risk.
  - Code ref: `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/lib.rs`.
  - Acceptance: per-command modules (`pkg`, `vcs`, `gc`, `sync`, `policy`, `store`, `gcpm`) plus stable JSON contract tests.
  - Completion (2026-02-19): extracted `pkg`, `vcs`, `gc`, `sync`, `policy`, `store`, and `refs` handlers into dedicated modules (`/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/cmd_pkg.rs`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/cmd_vcs.rs`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/cmd_gc.rs`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/cmd_sync.rs`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/cmd_policy.rs`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/cmd_store.rs`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/cmd_refs.rs`) and validated CLI contract parity via targeted domain suites.

- [x] Add policy-configurable byte budgets for `io/fs::read`, `core/store::get`, and sync pull batches.
  - Risk: capability caller can request arbitrarily large payloads even when op is allowlisted.
  - Code refs: `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner.rs:7044`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner.rs:3761`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner.rs:9879`.
  - Acceptance: per-op max read bytes + deterministic error code (`core/caps/resource-limit`) + replay-safe behavior.

- [x] Add reproducible microbench + regression budget gates for hot paths.
  - Targets: parser/canonicalizer, evaluator, effect runner step cost, sync throughput, lock/install/update flows.
  - Acceptance: CI job enforcing budgets with trend artifacts and fail thresholds.

- [x] Add warm-cache daemon/session mode for repeated CLI invocations in agent loops.
  - Goal: avoid repeated bootstrap/parse/canonicalize overhead for high-frequency AI workflows.
  - Acceptance: opt-in long-lived mode with deterministic cache keys and parity tests versus cold execution.
