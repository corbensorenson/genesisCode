# GenesisCode Upgrade Plan — Final Roadblocks to Full Self-Hosted Status

Last updated: 2026-02-18

## Hard Target
Ship a state where:
- All language/tooling semantics evolve in `.gc`.
- Rust is a frozen host runtime + kernel TCB, not the place where feature logic is added.
- New features (including graphics/editor/AI workflows) can be shipped by changing `.gc` + policy/docs, without touching Rust semantic code.

## What "Fully Self-Hosted" Means Here
Fully self-hosted for GenesisCode does **not** mean deleting all Rust binaries. It means:
1. Rust remains only as TCB/host bridge.
2. Command semantics, package/VCS logic, and developer workflows are owned by `.gc` contracts.
3. Selfhost-only mode covers all public command families needed for production use.
4. Toolchain growth (new selfhost modules/capabilities) does not require Rust edits.

## Audit Findings (Current Blocking Evidence)
- High-level package/VCS/GC/GPK semantics still execute in Rust capability code:
  - `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner.rs`
  - `core/pkg::*`, `core/vcs::*`, `core/gc::*`, `core/gpk::*`, `core/sync::*` are implemented there.
- `.gc` CLI wrappers still delegate those high-level ops to the Rust runner:
  - `/Users/corbensorenson/Documents/genesisCode/selfhost/cli_coreform_v1.gc`
- Selfhost-only command-family gating has been widened and planners for additional command families are now selfhost-routed:
  - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/lib.rs`
  - planner bindings now used in selfhost frontend path: `selfhost-artifact`, `keygen`, `sign`, `transparency-verify`, `verify`.
- Selfhost toolchain module set is now manifest-driven from:
  - `/Users/corbensorenson/Documents/genesisCode/selfhost/toolchain_manifest.gc`
  - loader validation enforces required symbols declared in the manifest.
- Shipped prelude wrappers now map to explicit runner dispatch entries, currently returning deterministic `core/caps/not-supported` where host integrations are not yet available:
  - wrappers in `/Users/corbensorenson/Documents/genesisCode/prelude/modules/10_gfx.gc` and `/Users/corbensorenson/Documents/genesisCode/prelude/modules/20_editor.gc`
  - dispatch in `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner.rs` and ABI lock in `/Users/corbensorenson/Documents/genesisCode/docs/spec/HOST_ABI.md`.
- GFX obligations still encode major logic in Rust:
  - `/Users/corbensorenson/Documents/genesisCode/crates/gc_obligations/src/lib.rs`
  - `obligation_gfx_golden_images`, `obligation_gfx_frame_budgets`, `obligation_gfx_api_stability`.

---

## Final Blocking Workstreams

### 1) Extract High-Level Semantics out of Rust Runner
- [ ] Move `core/pkg::*` command semantics into `.gc` contracts using low-level host capabilities.
- [ ] Move `core/vcs::*` command semantics into `.gc` contracts using low-level host capabilities.
  - [x] Move `core/vcs::log` traversal semantics into `.gc` (`core/cli::vcs-log-program`) using `core/store::get` / `core/refs::get`.
  - [x] Move `core/vcs::blame` and `core/vcs::why` provenance semantics into `.gc` (`core/cli::vcs-blame-program`, `core/cli::vcs-why-program`) using `core/store::get` / `core/refs::list`.
  - [x] Add native + WASI parity tests for `core/vcs::diff` and `core/vcs::apply` frontend outputs to lock extraction behavior before migration.
  - [x] Add native + WASI parity tests for `core/vcs::merge3` and `core/vcs::resolve-conflict` frontend outputs to lock extraction behavior before migration.
  - [x] Lock and document the remaining Rust-owned high-level op inventory (`core/vcs::{diff,apply,merge3,resolve-conflict}` and `core/pkg::*`) as the current extraction queue.
  - [ ] Move `core/vcs::diff` and `core/vcs::apply` semantics into `.gc` contracts.
  - [ ] Move `core/vcs::merge3` and `core/vcs::resolve-conflict` semantics into `.gc` contracts.
  - [ ] Move `core/pkg::{init,add,list,info,lock,update,install,verify,snapshot,publish}` semantics into `.gc` contracts.
- [x] Move `core/gc::*` and `core/gpk::*` planning/closure logic into `.gc`.
- [ ] Reduce Rust runner capability surface to low-level host ops (`core/store::*`, `core/refs::*`, `core/sync::*`, `io/fs::*`, `sys/time::now`) plus transport glue.
- [x] Keep temporary compatibility gate for migration, then disable by default.
- [x] Extract GPK artifact-reference reachability semantics to `.gc` (`core/vcs/reach::artifact-refs`) and invoke from Rust closure traversal.
- [x] Extract GPK parent-edge planning semantics to `.gc` (`core/vcs/reach::artifact-ref-plan`) and consume parent refs from plan output in Rust traversal.

Acceptance:
- Rust runner no longer contains semantic implementations for `core/pkg::*`, `core/vcs::*`, `core/gc::*`, `core/gpk::*`.
- Equivalent strict parity tests pass on native + WASI selfhost paths.

### 2) Close Selfhost-Only Command Surface Gaps
- [x] Route `selfhost-artifact` through selfhost contract path.
- [x] Route `keygen`, `sign`, `transparency-verify`, and `verify` through selfhost contract path.
- [x] Update selfhost-only allowlist to include all production public command families.
- [x] Add native + WASI selfhost-only tests for these command families.

Acceptance:
- `--selfhost-only` rejects no production command family as "not yet selfhost-routed".

### 3) Make Toolchain Bootstrap Self-Describing (No Rust Module List Edits)
- [x] Replace hardcoded Rust `MODULE_SOURCES` ownership with a `.gc` toolchain manifest artifact.
- [x] Make artifact loader validate required capabilities/symbols from manifest, not a Rust static list.
- [x] Ensure adding a new selfhost module only requires `.gc` + manifest updates.
- [x] Keep embedded bootstrap as development-only fallback behind explicit feature gate.

Acceptance:
- Toolchain module topology changes can be made without editing Rust source files.

### 4) Implement Missing GFX/Editor Capability Ops
- [x] Implement runner support (or remove wrappers) for shipped `gfx/gpu::*` ops.
- [x] Implement runner support (or remove wrappers) for shipped `gfx/window::*` ops.
- [x] Implement runner support (or remove wrappers) for shipped `gfx/input::*` and `gfx/audio::*` ops.
- [x] Implement runner support (or remove wrappers) for shipped `editor/*` task/dialog/watch/clipboard ops.
- [x] Add deterministic effect-log + replay tests for all newly supported ops.

Acceptance:
- No shipped prelude capability wrapper calls an unimplemented/unknown op.

### 5) Move GFX Obligation Semantics to `.gc` Ownership
- [x] Port golden/frame-budget/api-stability planning/validation logic to `.gc` contracts.
- [ ] Keep Rust role limited to host execution + artifact persistence + capability transport.
- [x] Add parity tests ensuring `.gc` obligation outputs are deterministic and stable.

Acceptance:
- GFX obligation behavior changes can be shipped by editing `.gc`, not Rust algorithms.

### 6) Freeze/Archive Rust Compatibility Paths for Production
- [x] Make Rust frontend and embedded fallback strictly compatibility-only and off in production defaults.
- [x] Move non-essential compatibility semantic code into `/old_bootstrap` where practical.
- [x] Lock selfhost boundary policy in CI to prevent semantic creep back into Rust.
- [x] Publish explicit host ABI freeze doc for post-cutover governance.

Acceptance:
- Production profile runs selfhost paths only; Rust semantic compatibility requires explicit opt-in.

---

## Critical Path Order
1. Workstream 1 (semantic extraction from runner)
2. Workstream 3 (self-describing bootstrap)
3. Workstream 2 (full selfhost-only command coverage)
4. Workstream 4 (capability completeness for shipped gfx/editor wrappers)
5. Workstream 5 (gfx obligation ownership)
6. Workstream 6 (final freeze/archive)

---

## Exit Criteria for "Fully Self-Hosted Core"
- [x] Selfhost-only mode covers all production command families.
- [ ] No high-level package/VCS/GC/GPK semantic logic remains in Rust runner.
- [x] Toolchain module graph can evolve without Rust source edits.
- [x] Shipped prelude capability wrappers are all backed by implemented host ops or removed.
- [x] Rust compatibility paths are non-default and clearly isolated.

---

## Immediate Post-Cutover Queue (AI-First, Not Blocking Self-Host)
- [x] Optimize selfhost pipeline throughput (incremental graph + cache + hot path budgets).
- [x] Standardize machine-first diagnostics schema across all commands (stable fields, deterministic ordering, no free-form drift).
- [x] Expand AI-oriented editing/provenance primitives (semantic patch planning, conflict resolution helpers, obligation-guided repair loops).
- [x] Harden graphics/editor AI workflows (task orchestration contracts, deterministic replayable UI/GPU traces, artifact-linked explainability).
