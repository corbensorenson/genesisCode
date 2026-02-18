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
- Selfhost-only still rejects several public commands:
  - `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/lib.rs` (`enforce_selfhost_only_cmd`)
  - currently unsupported in selfhost-only: `selfhost-artifact`, `keygen`, `sign`, `transparency-verify`, `verify`.
- Selfhost toolchain module set is hardcoded in Rust:
  - `/Users/corbensorenson/Documents/genesisCode/crates/gc_prelude/src/selfhost_coreform_v1.rs` (`MODULE_SOURCES`)
  - adding/removing toolchain modules still requires Rust edits.
- Shipped prelude wrappers expose gfx/editor capability ops that the runner does not implement:
  - wrappers in `/Users/corbensorenson/Documents/genesisCode/prelude/modules/10_gfx.gc` and `/Users/corbensorenson/Documents/genesisCode/prelude/modules/20_editor.gc`
  - missing in runner op table except `gfx/time::frame-tick`.
- GFX obligations still encode major logic in Rust:
  - `/Users/corbensorenson/Documents/genesisCode/crates/gc_obligations/src/lib.rs`
  - `obligation_gfx_golden_images`, `obligation_gfx_frame_budgets`, `obligation_gfx_api_stability`.

---

## Final Blocking Workstreams

### 1) Extract High-Level Semantics out of Rust Runner
- [ ] Move `core/pkg::*` command semantics into `.gc` contracts using low-level host capabilities.
- [ ] Move `core/vcs::*` command semantics into `.gc` contracts using low-level host capabilities.
- [ ] Move `core/gc::*` and `core/gpk::*` planning/closure logic into `.gc`.
- [ ] Reduce Rust runner capability surface to low-level host ops (`core/store::*`, `core/refs::*`, `core/sync::*`, `io/fs::*`, `sys/time::now`) plus transport glue.
- [ ] Keep temporary compatibility gate for migration, then disable by default.

Acceptance:
- Rust runner no longer contains semantic implementations for `core/pkg::*`, `core/vcs::*`, `core/gc::*`, `core/gpk::*`.
- Equivalent strict parity tests pass on native + WASI selfhost paths.

### 2) Close Selfhost-Only Command Surface Gaps
- [ ] Route `selfhost-artifact` through selfhost contract path.
- [ ] Route `keygen`, `sign`, `transparency-verify`, and `verify` through selfhost contract path.
- [ ] Update selfhost-only allowlist to include all production public command families.
- [ ] Add native + WASI selfhost-only tests for these command families.

Acceptance:
- `--selfhost-only` rejects no production command family as "not yet selfhost-routed".

### 3) Make Toolchain Bootstrap Self-Describing (No Rust Module List Edits)
- [ ] Replace hardcoded Rust `MODULE_SOURCES` ownership with a `.gc` toolchain manifest artifact.
- [ ] Make artifact loader validate required capabilities/symbols from manifest, not a Rust static list.
- [ ] Ensure adding a new selfhost module only requires `.gc` + manifest updates.
- [ ] Keep embedded bootstrap as development-only fallback behind explicit feature gate.

Acceptance:
- Toolchain module topology changes can be made without editing Rust source files.

### 4) Implement Missing GFX/Editor Capability Ops
- [ ] Implement runner support (or remove wrappers) for shipped `gfx/gpu::*` ops.
- [ ] Implement runner support (or remove wrappers) for shipped `gfx/window::*` ops.
- [ ] Implement runner support (or remove wrappers) for shipped `gfx/input::*` and `gfx/audio::*` ops.
- [ ] Implement runner support (or remove wrappers) for shipped `editor/*` task/dialog/watch/clipboard ops.
- [ ] Add deterministic effect-log + replay tests for all newly supported ops.

Acceptance:
- No shipped prelude capability wrapper calls an unimplemented/unknown op.

### 5) Move GFX Obligation Semantics to `.gc` Ownership
- [ ] Port golden/frame-budget/api-stability planning/validation logic to `.gc` contracts.
- [ ] Keep Rust role limited to host execution + artifact persistence + capability transport.
- [ ] Add parity tests ensuring `.gc` obligation outputs are deterministic and stable.

Acceptance:
- GFX obligation behavior changes can be shipped by editing `.gc`, not Rust algorithms.

### 6) Freeze/Archive Rust Compatibility Paths for Production
- [ ] Make Rust frontend and embedded fallback strictly compatibility-only and off in production defaults.
- [ ] Move non-essential compatibility semantic code into `/old_bootstrap` where practical.
- [ ] Lock selfhost boundary policy in CI to prevent semantic creep back into Rust.
- [ ] Publish explicit host ABI freeze doc for post-cutover governance.

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
- [ ] Selfhost-only mode covers all production command families.
- [ ] No high-level package/VCS/GC/GPK semantic logic remains in Rust runner.
- [ ] Toolchain module graph can evolve without Rust source edits.
- [ ] Shipped prelude capability wrappers are all backed by implemented host ops or removed.
- [ ] Rust compatibility paths are non-default and clearly isolated.

---

## Immediate Post-Cutover Queue (AI-First, Not Blocking Self-Host)
- [ ] Optimize selfhost pipeline throughput (incremental graph + cache + hot path budgets).
- [ ] Standardize machine-first diagnostics schema across all commands (stable fields, deterministic ordering, no free-form drift).
- [ ] Expand AI-oriented editing/provenance primitives (semantic patch planning, conflict resolution helpers, obligation-guided repair loops).
- [ ] Harden graphics/editor AI workflows (task orchestration contracts, deterministic replayable UI/GPU traces, artifact-linked explainability).

