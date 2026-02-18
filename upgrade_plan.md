# GenesisCode Upgrade Plan — Self-Hosted v1 + GCPM Fast Path

Last updated: 2026-02-18

This plan now contains only unfinished work. Completed checklist items were removed.

Open checklist items: 7

## Findings From Current Project Audit

- The foundation is strong: Genesis already ships integrated `store`, `refs`, `sync`, `vcs`, `pkg`, `policy`, and obligation pipelines in the CLI/runtime.
- Current package management is lock-centric (`genesis.lock`) and package-centric (`package.toml`), but not yet workspace-native in the way Cargo/Pixi users expect.
- Environment setup is not yet modeled as a first-class, deterministic workspace artifact (toolchain/deps/profile environment closure).
- High-level semantic dispatch for package/VCS/GC/GPK still exists in Rust capability runner; full self-host cutover remains open.
- Selfhost-only enforcement is strong, but production-grade removal/relocation of bootstrap Rust semantics is not completed yet.

## Naming Decision (Project Manager)

- [x] Adopt `GCPM` (GenesisCode Project Manager) as product name.
- [x] Keep `genesis pkg` as stable compatibility surface and add `genesis gcpm` as first-class alias.
- [x] Freeze command naming and JSON output contracts for AI agents (no churn without schema version bump).

## Workstream A — Self-Host Completion Blockers

- [ ] All production command semantics are owned by `.gc` contracts.
- [ ] Rust runtime is limited to kernel + low-level host ABI + transport.
- [x] Move `core/pkg::snapshot` semantics fully into `.gc` contracts (host only provides low-level capabilities).
- [x] Move `core/pkg::publish` semantics fully into `.gc` contracts (closure planning, policy prechecks, reports).
- [x] Remove remaining high-level `core/pkg::*` execution branches from `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner.rs` after parity lock.
- [ ] Remove remaining high-level `core/vcs::*`, `core/gc::*`, and `core/gpk::*` execution branches from `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner.rs` after low-level seam parity.
- [ ] Keep Rust capability surface to low-level ops only: `core/store::*`, `core/refs::*`, `core/sync::*`, `io/fs::*`, `sys/time::now`, graphics/editor host ops.
- [x] Complete Stage-2 selfhost path so toolchain evolution is authored and validated in Genesis code first (enforced by `scripts/check_selfhost_artifact_fresh.sh` + `scripts/test_fast.sh`).
- [ ] Remove production fallback to Rust semantic implementations once parity + replay + obligation gates pass.
- [x] In `--selfhost-only` mode, fail fast when runtime logs show legacy high-level `core/pkg::*` or `core/vcs::*` semantic ops (prevents production fallback drift).
- [ ] Move remaining bootstrap-only Rust semantic code under `/Users/corbensorenson/Documents/genesisCode/old_bootstrap` after cutover.
- [x] Package publish/install workflows are fully `.gc`-owned semantics with low-level host caps only.

## Workstream B — GCPM Core (Language-Native Project Manager)

### B1. Workspace Model
- [x] Define and implement `genesis.workspace.toml` (or canonical CoreForm equivalent) as workspace root descriptor.
- [x] Add multi-package workspace graph support (members, local paths, package roles).
- [x] Add workspace-level policy/default registry/toolchain/profile declarations.

### B2. Lock + Resolution v2
- [x] Extend `genesis.lock` to workspace-scoped deterministic lock v2 with per-package resolved snapshots/commits and environment fingerprints.
- [x] Add deterministic resolver strategy modes (`pinned`, `track-ref`, `tag-policy`) with strict lock pinning.
- [x] Add lock drift diagnostics with canonical AI-fix metadata (actionable by agents).

### B3. Environment Automation (Pixi-like UX, deterministic)
- [x] Add `gcpm env` surface for deterministic environment realization from lock + toolchain pins.
- [x] Materialize environment artifacts under `.genesis/env/<profile-hash>/` with immutable provenance records.
- [x] Add profile support (`dev`, `ci`, `release`) with policy-gated capability surfaces.

### B4. Command Surface (AI-First)
- [x] Add `genesis gcpm init/new/add/remove/lock/install/update/run/test/publish/info/list`.
- [x] Implement `gcpm run <task>` with workspace tasks as canonical data (not ad hoc shell glue).
- [x] Ensure every `gcpm` command has stable JSON schema + deterministic machine-readable diagnostics.

### B5. In-Language Contract Surface
- [x] Define `core/pm::*` contract API so AI can drive project management from Genesis code, not only CLI.
- [x] Keep state-mutating `core/pm::*` operations effectful and replay-logged.
- [x] Add obligation/policy gates to `core/pm::publish`, `core/pm::update`, `core/pm::lock`.

## Workstream C — VCS + PM Unification

- [x] Make workspace/project state snapshots first-class `:vcs/snapshot` roots.
- [x] Bind `gcpm lock/install/update/publish` operations to explicit commit/evidence provenance edges.
- [x] Add branch-aware dependency tracking semantics (`track ref + locked commit`) at workspace level.
- [x] Add deterministic migration path from package-only mode to workspace+gcpm mode.

## Workstream D — AI-First Developer Experience

- [x] Add canonical diagnostic/fix schema docs for `gcpm` errors and resolver conflicts.
- [x] Add AI-optimized “what changed / why / fix options” report artifacts for lock/update/publish workflows.
- [x] Add deterministic “project doctor” command (`gcpm doctor`) with policy + lock + capability checks.
- [x] Add prompt-safe command telemetry artifacts (non-sensitive, deterministic summaries) for agent loops.

## Acceptance Checks (Must Pass Before v1 Declaration)

- [x] `--selfhost-only` + `gcpm` executes full workspace lifecycle (init/add/lock/install/run/test/publish) with no Rust semantic fallback.
- [x] End-to-end workspace operations are replayable and policy-gated with deterministic logs.
- [x] Lock v2 + environment realization meets AI iteration targets in CI budget checks.
- [ ] Rust semantic bootstrap code is relocated to `/Users/corbensorenson/Documents/genesisCode/old_bootstrap` and no longer used in production path.
