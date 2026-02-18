# GenesisCode Upgrade Plan — Fast Path to Fully Self-Hosted Core

Last updated: 2026-02-17

## Objective
Ship a fully self-hosted GenesisCode core as quickly as possible, then move Rust bootstrap semantics out of the active path.
AI-first constraint: command surfaces and diagnostics must remain deterministic, machine-parseable, and easy for autonomous agents to drive end-to-end.

## Definition of Done (Fast Path)
A fast-path cutover is complete when all of the following are true:
- Default `genesis` execution for core workflows runs through self-hosted `.gc` toolchain paths.
- Rust is not the active source of language semantics in default profile.
- `--selfhost-only` passes on native and WASI for core workflows with deterministic parity checks.
- Legacy Rust bootstrap semantics are moved to `/old_bootstrap` and excluded from default builds/tests.

## Current Status Snapshot
- Core strict-mode guardrails are in place and heavily tested on native + WASI.
- Core command parity coverage has expanded significantly (including `optimize`, `apply-patch`, and `selfhost-dashboard` on WASI).
- Strict smoke and strict golden now enforce cross-host/cross-engine parity on key paths.
- Main blockers are now structural: `.gc` command contract ownership, `.gc` semantic ownership of toolchain passes, and bootstrap extraction.
- Rust-vs-selfhost frontend parity for package/obligation/patch flows is now explicit via `--coreform-frontend`.
- Strict full-cutover rehearsal scripts (`selfhost_strict_smoke` + `selfhost_strict_golden`) are passing on native + WASI.
- Native + WASI `typecheck` command paths now call a shared `gc_obligations` implementation, removing duplicated CLI semantics for that command family.
- Selfhost `core/cli` now owns module `::meta` extraction via `core/cli::module-meta`, and obligations typecheck prefers that contract path.
- Native + WASI parity tests now assert selfhost typecheck fails deterministically when `core/cli::module-meta` is poisoned in generated artifacts.
- Native + WASI `optimize` command semantics now run through shared `gc_opt::optimize_command_pipeline` (stage1/stage2/emit-wasm gating unified).
- Obsolete CLI-local optimize JSON helper logic is now centralized in `gc_opt` and shared by native + WASI paths.
- Native + WASI `optimize --json` now emits explicit `coreform_frontend` provenance for deterministic AI-agent orchestration parity with `test`/`pack`/`typecheck`/`apply-patch`.
- Selfhost frontend module loading now prefers `core/cli::hash-module-forms` for module hash derivation (with deterministic failure tests on native + WASI when poisoned).
- Native + WASI `selfhost-artifact` rebuild now parses/canonicalizes/hashes toolchain modules through selfhost frontend contracts (embedded bootstrap), removing Rust parser/hash semantics from artifact rebuild path.

---

## Completed Baseline (Condensed)
- [x] Self-host boundary and CI guardrails are defined/enforced.
- [x] `--selfhost-only` hard mode exists for native + WASI and blocks non-compliant routes.
- [x] Default profile blocks `--engine rust` unless compatibility env (`GENESIS_ALLOW_RUST_ENGINE=1`) is explicitly set.
- [x] Native + WASI strict selfhost smoke/golden suites are active.
- [x] Native + WASI parity checks cover `fmt`, `eval`, `run`, `replay`, `typecheck`, `optimize`, `pack`, `test`, `apply-patch`, `vcs hash`.
- [x] WASI command-surface parity was expanded for missing gaps (`explain`, `typecheck`, `optimize`, `apply-patch`, `selfhost-dashboard`, `pkg publish`).
- [x] `selfhost-dashboard` command exists on native + WASI, with markdown/store output checks.
- [x] `gc_obligations` and `gc_patches` default frontend paths now prefer selfhost frontend.
- [x] `selfhost-strict` CI profile is enabled by default and validated.

---

## Fast Path Workstreams

### A) `.gc` CLI Contract Ownership (Highest Priority)
- [x] Define initial `core/cli::*` contract interface for frontend parse/canon/fmt/hash.
- [x] Route core commands through `.gc` handlers by default:
  - [x] `fmt`, `eval` route through `core/cli::*` frontend handlers (with compatibility fallback).
  - [x] `test`, `typecheck`, `optimize`, `pack`, `apply-patch` selfhost frontend paths now prefer `core/cli::*` canonicalization handlers.
- [x] Route effectful command groups through `.gc` command contracts:
  - [x] Incremental: `store/*` now routes through `core/cli::store-*-program` when `--coreform-frontend selfhost` is active, with native+WASI parity + poison tests.
  - [x] Incremental: `refs/*` now routes through `core/cli::refs-*-program` when `--coreform-frontend selfhost` is active, with native+WASI parity + poison tests.
  - [x] Incremental: `sync/*` now routes through `core/cli::sync-*-program` when `--coreform-frontend selfhost` is active, with native+WASI poison tests.
  - [x] Incremental: `gc/*` now routes through `core/cli::gc-*-program` when `--coreform-frontend selfhost` is active, with native+WASI poison tests.
  - [x] Incremental: `vcs hash` now prefers `core/cli::hash-src-with-kind` (with compatibility fallback).
  - [x] Incremental: `vcs/*` now routes through `core/cli::vcs-*-program` when `--coreform-frontend selfhost` is active, with native+WASI parity + poison tests.
- [x] Incremental: `pkg/*` now routes through `core/cli::pkg-*-program` when `--coreform-frontend selfhost` is active, with native+WASI parity + poison tests.
- [x] Reduce Rust CLI to arg parsing + host bridge only.
- [x] Keep selfhost artifact in sync with `core/cli` module surface and enforce via native+WASI regression tests.
- [x] Add explicit `--coreform-frontend {rust,selfhost}` selector for package/obligation/patch paths to support deterministic AI parity checks.

Acceptance gate:
- [x] CLI golden parity proves old Rust command logic and `.gc` command contracts are behavior-identical for covered paths.

### B) `.gc` Semantic Source-of-Truth
- [x] Finalize self-host parser/canon/printer/hash as canonical source of truth.
  - [x] Remove legacy `selfhost/tool::*` fallback bindings from the CLI driver; selfhost routes require `core/cli::*` contracts.
  - [x] Toolchain artifact loader enforces canonical `:forms` (idempotent canonicalization) and hashes canonical forms to prevent semantic skew.
- [ ] Implement self-host Stage-1 transform pipeline in `.gc`.
- [ ] Implement self-host type/effect checker in `.gc` and wire to `core/obligation::typecheck`.
- [ ] Implement self-host optimizer pipeline in `.gc` and wire to translation-validation obligation.
- [ ] Implement self-host patch schema validation/apply pipeline in `.gc`.
  - [x] Add `selfhost/patch_schema_v1.gc` and expose `core/cli::validate-patch` in the selfhost toolchain.
  - [x] Enforce patch schema acceptance via `core/cli::validate-patch` when `--coreform-frontend selfhost` is active (host still applies patch ops in Rust for now).
- [x] Guarantee byte-for-byte deterministic artifacts/evidence for selfhost paths.
  - [x] `selfhost-artifact` output is byte-for-byte deterministic across rebuilds on the same toolchain (enforced by `gc_cli` tests).
  - [x] `pack` and `test` acceptance artifact hashes are deterministic across reruns under the selfhost frontend (enforced by `gc_cli` tests).
  - [x] `.gpk` export output is byte-for-byte deterministic for the same root snapshot and store state (enforced by `gc_cli` tests).
  - [x] `pkg lock` / `pkg update` are deterministic for the same store+refs state (enforced by `gc_cli` + `gc_wasi_cli` tests).
  - [x] `apply-patch` artifact outputs are deterministic across reruns under the selfhost frontend (enforced by `gc_cli` tests).

Acceptance gate:
- [ ] Native + WASI parity suites remain green when Rust semantic fallbacks are removed from default path.

### C) Bootstrap Extraction (`/old_bootstrap`)
- [x] Move replaced Rust semantic bootstrap modules to `/old_bootstrap`.
- [x] Exclude `/old_bootstrap` from default build/test paths.
- [x] Keep compatibility profile for historical comparisons only.

Acceptance gate:
- [x] `cargo test --workspace --profile selfhost-strict` passes without invoking bootstrap semantics from `/old_bootstrap`.

### D) Final Cutover Proof
- [x] End-to-end workspace flow (`pkg add/lock/install/test/publish/export/import`) passes via selfhost-first paths.
- [x] Toolchain artifact can be rebuilt from `.gc` sources (host bridge allowed, no Rust semantic dependency).
- [x] Cutover dashboard and CI checks confirm selfhost default path is authoritative.

---

## Task List (Current Execution Queue)
- [x] 1) Implement `core/cli::*` interface in `.gc` and wire `fmt/eval` through it.
- [x] 2) Regenerate `selfhost/toolchain.gc` and add native+WASI regression tests that require `selfhost/cli_coreform_v1.gc` with passing stage1 gate.
- [x] 3) Add explicit `--coreform-frontend {rust,selfhost}` selector for package/obligation/patch commands, plus strict-mode guard tests.
- [x] 4) Extend `core/cli::*` routing to `test/typecheck/optimize/pack/apply-patch` command-owned handlers (not only shared frontend canonicalization).
- [x] 5) Add CLI parity goldens that compare legacy Rust route vs `.gc` contract route for the command set above.
- [x] 6) Remove duplicated Rust command semantics once parity gate is green.
  - [x] 6a) Deduplicate native + WASI `typecheck` command semantics by routing through `gc_obligations::typecheck_package_with_step_limit_and_frontend`.
  - [x] 6b) Deduplicate `pack` + `apply-patch` command families via shared `gc_obligations` / `gc_patches` entrypoints on native + WASI.
  - [x] 6c) Deduplicate remaining command families (`test`, `optimize`) where CLI-local semantic duplication still exists.
  - [x] 6d) Remove obsolete CLI-only helper code after each family is migrated and covered by parity tests.
- [ ] 7) Complete `.gc` stage1/typecheck/optimize/patch ownership and switch obligations to those paths.
  - [x] 7a) Typecheck-prep path now prefers selfhost `core/cli::module-meta` contract for module metadata extraction.
  - [x] 7b) Module-loading hash derivation now prefers selfhost `core/cli::hash-module-forms` instead of Rust-only hashing in selfhost frontend paths.
- [x] 8) Move replaced Rust semantic modules to `/old_bootstrap` and enforce default exclusion.
- [x] 9) Run strict full cutover rehearsal (native + WASI) and freeze.
- [x] 10) Add explicit `coreform_frontend` provenance fields in JSON outputs (`test`, `pack`, `typecheck`, `apply-patch`) for deterministic AI-agent orchestration.
- [x] 11) Strengthen strict smoke/golden parity harnesses to force explicit `--coreform-frontend rust|selfhost` selection for package/obligation/patch command families.
- [x] 12) Extend `optimize --json` outputs with explicit `coreform_frontend` provenance on native + WASI and lock via parity tests.
- [x] 13) Route `selfhost-artifact` parse/canon/hash through selfhost frontend contracts so rebuilds do not depend on Rust parser/hash semantics.

### Execution Sprint (Now)
- [x] T1: Route native + WASI `genesis typecheck` through shared obligations implementation and remove duplicate per-CLI logic.
- [x] T2: Enforce clean build quality for this migration (`cargo fmt`, targeted tests, and `clippy -D warnings` for native + WASI CLIs).
- [x] T3: Confirm `genesis pack` uses shared `gc_obligations::pack_with_frontend` path on native + WASI (no duplicated CLI semantics).
- [x] T4: Confirm `genesis apply-patch` uses shared `gc_patches::apply_patch_with_step_limit_and_frontend` path on native + WASI (no duplicated CLI semantics).
- [x] T5: Start `.gc` semantic ownership migration for typecheck obligation path beyond shared Rust wrapper (module metadata now extracted through `core/cli::module-meta` when selfhost frontend is active).

### Execution Sprint (Next)
- [x] N1: Deduplicate `optimize` command family into a shared library path for native + WASI.
- [x] N2: Deduplicate any remaining `test` CLI-local semantic logic into shared library path.
- [x] N3: Add parity tests asserting `core/cli::module-meta` contract path is active for generated selfhost artifacts.

---

## Non-Blocking Backlog (Post Fast-Path Cutover)
These are important but not blockers for the fast-path self-hosted core milestone:
- [ ] Full `.gc` implementation of GenesisGraph constructors/validators/reachability planner internals.
- [ ] Full `.gc` lock/resolver and `.gpk` planner internals (beyond current runtime parity coverage).
- [ ] Second host implementation conformance harness for host ABI portability proof.
- [ ] `.gc` profiling/incremental build graph/perf acceleration wave.
- [ ] Graphics/WebGPU/editor stack and higher-level developer UX layers.

---

## Estimate (From Current State)
- Fast-path fully self-hosted core: ~7-12 days of focused work.
- Full original upgrade scope (including ecosystem backlog above): multi-week to multi-month.
