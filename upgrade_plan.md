# GenesisCode Upgrade Plan — Fast Path to Fully Self-Hosted Core

Last updated: 2026-02-17

## Objective
Ship a fully self-hosted GenesisCode core as quickly as possible, then move Rust bootstrap semantics out of the active path.

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
- [ ] Define `core/cli::*` contract interface as the canonical command contract surface.
- [ ] Route core commands through `.gc` handlers by default:
  - [ ] `fmt`, `eval`, `test`, `typecheck`, `optimize`, `pack`, `apply-patch`
- [ ] Route effectful command groups through `.gc` command contracts:
  - [ ] `store/*`, `refs/*`, `vcs/*`, `pkg/*`, `policy/*`, `sync/*`, `gc/*`
- [ ] Reduce Rust CLI to arg parsing + host bridge only.

Acceptance gate:
- [ ] CLI golden parity proves old Rust command logic and `.gc` command contracts are behavior-identical for covered paths.

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

## Immediate Next Pushes (Fast Path)
- [ ] 1) Implement `core/cli::*` interface in `.gc` and wire `fmt/eval` through it.
- [ ] 2) Extend `core/cli::*` routing to `test/typecheck/optimize/pack/apply-patch`.
- [ ] 3) Add CLI golden tests that compare Rust route vs `.gc` contract route for the command set above.
- [ ] 4) Remove duplicated Rust command semantics once parity gate is green.
- [ ] 5) Complete `.gc` stage1/typecheck/optimize/patch ownership and switch obligations to those paths.
- [ ] 6) Move replaced Rust semantic modules to `/old_bootstrap` and enforce default exclusion.
- [ ] 7) Run strict full cutover rehearsal (native + WASI) and freeze.

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
