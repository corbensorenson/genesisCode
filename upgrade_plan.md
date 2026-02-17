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
- [ ] Route core commands through `.gc` handlers by default:
  - [x] `fmt`, `eval` route through `core/cli::*` frontend handlers (with compatibility fallback).
  - [x] `test`, `typecheck`, `optimize`, `pack`, `apply-patch` selfhost frontend paths now prefer `core/cli::*` canonicalization handlers.
- [ ] Route effectful command groups through `.gc` command contracts:
  - [ ] `store/*`, `refs/*`, `vcs/*`, `pkg/*`, `policy/*`, `sync/*`, `gc/*`
  - [x] Incremental: `vcs hash` now prefers `core/cli::hash-src-with-kind` (with compatibility fallback).
- [ ] Reduce Rust CLI to arg parsing + host bridge only.
- [x] Keep selfhost artifact in sync with `core/cli` module surface and enforce via native+WASI regression tests.
- [x] Add explicit `--coreform-frontend {rust,selfhost}` selector for package/obligation/patch paths to support deterministic AI parity checks.

Acceptance gate:
- [x] CLI golden parity proves old Rust command logic and `.gc` command contracts are behavior-identical for covered paths.

### B) `.gc` Semantic Source-of-Truth
- [ ] Finalize self-host parser/canon/printer/hash as canonical source of truth.
- [ ] Implement self-host Stage-1 transform pipeline in `.gc`.
- [ ] Implement self-host type/effect checker in `.gc` and wire to `core/obligation::typecheck`.
- [ ] Implement self-host optimizer pipeline in `.gc` and wire to translation-validation obligation.
- [ ] Implement self-host patch schema validation/apply pipeline in `.gc`.
- [ ] Guarantee byte-for-byte deterministic artifacts/evidence for selfhost paths.

Acceptance gate:
- [ ] Native + WASI parity suites remain green when Rust semantic fallbacks are removed from default path.

### C) Bootstrap Extraction (`/old_bootstrap`)
- [ ] Move replaced Rust semantic bootstrap modules to `/old_bootstrap`.
- [ ] Exclude `/old_bootstrap` from default build/test paths.
- [ ] Keep compatibility profile for historical comparisons only.

Acceptance gate:
- [ ] `cargo test --workspace --profile selfhost-strict` passes without invoking bootstrap semantics from `/old_bootstrap`.

### D) Final Cutover Proof
- [ ] End-to-end workspace flow (`pkg add/lock/install/test/publish/export/import`) passes via selfhost-first paths.
- [ ] Toolchain artifact can be rebuilt from `.gc` sources (host bridge allowed, no Rust semantic dependency).
- [ ] Cutover dashboard and CI checks confirm selfhost default path is authoritative.

---

## Task List (Current Execution Queue)
- [x] 1) Implement `core/cli::*` interface in `.gc` and wire `fmt/eval` through it.
- [x] 2) Regenerate `selfhost/toolchain.gc` and add native+WASI regression tests that require `selfhost/cli_coreform_v1.gc` with passing stage1 gate.
- [x] 3) Add explicit `--coreform-frontend {rust,selfhost}` selector for package/obligation/patch commands, plus strict-mode guard tests.
- [x] 4) Extend `core/cli::*` routing to `test/typecheck/optimize/pack/apply-patch` command-owned handlers (not only shared frontend canonicalization).
- [x] 5) Add CLI parity goldens that compare legacy Rust route vs `.gc` contract route for the command set above.
- [ ] 6) Remove duplicated Rust command semantics once parity gate is green.
  - [x] 6a) Deduplicate native + WASI `typecheck` command semantics by routing through `gc_obligations::typecheck_package_with_step_limit_and_frontend`.
  - [x] 6b) Deduplicate `pack` + `apply-patch` command families via shared `gc_obligations` / `gc_patches` entrypoints on native + WASI.
  - [ ] 6c) Deduplicate remaining command families (`test`, `optimize`) where CLI-local semantic duplication still exists.
  - [ ] 6d) Remove obsolete CLI-only helper code after each family is migrated and covered by parity tests.
- [ ] 7) Complete `.gc` stage1/typecheck/optimize/patch ownership and switch obligations to those paths.
  - [x] 7a) Typecheck-prep path now prefers selfhost `core/cli::module-meta` contract for module metadata extraction.
- [ ] 8) Move replaced Rust semantic modules to `/old_bootstrap` and enforce default exclusion.
- [x] 9) Run strict full cutover rehearsal (native + WASI) and freeze.
- [x] 10) Add explicit `coreform_frontend` provenance fields in JSON outputs (`test`, `pack`, `typecheck`, `apply-patch`) for deterministic AI-agent orchestration.
- [x] 11) Strengthen strict smoke/golden parity harnesses to force explicit `--coreform-frontend rust|selfhost` selection for package/obligation/patch command families.

### Execution Sprint (Now)
- [x] T1: Route native + WASI `genesis typecheck` through shared obligations implementation and remove duplicate per-CLI logic.
- [x] T2: Enforce clean build quality for this migration (`cargo fmt`, targeted tests, and `clippy -D warnings` for native + WASI CLIs).
- [x] T3: Confirm `genesis pack` uses shared `gc_obligations::pack_with_frontend` path on native + WASI (no duplicated CLI semantics).
- [x] T4: Confirm `genesis apply-patch` uses shared `gc_patches::apply_patch_with_step_limit_and_frontend` path on native + WASI (no duplicated CLI semantics).
- [x] T5: Start `.gc` semantic ownership migration for typecheck obligation path beyond shared Rust wrapper (module metadata now extracted through `core/cli::module-meta` when selfhost frontend is active).

### Execution Sprint (Next)
- N1: Deduplicate `optimize` command family into a shared library path for native + WASI.
- N2: Deduplicate any remaining `test` CLI-local semantic logic into shared library path.
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
