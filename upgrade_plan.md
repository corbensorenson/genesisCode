# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-24

Scope:
- List only unresolved blockers/risk-reduction work for selfhost closure and AI-first productization.
- Remove completed items; rely on git history and perf artifacts for closure evidence.
- Machine-readable source of unresolved IDs: `.genesis/perf/selfhost_readiness_report.json`.

Open checklist items: 3

## Selfhost Closure (Rust Ownership Still In Critical Path)

- [ ] P2.1 Move `crates/gc_obligations/src/obligation_exec.rs` from `phase-1 in-progress` to `phase-3 migrated` by routing production obligation orchestration through `.gc` ownership and archiving Rust semantic sidecars to parity-only paths.
- [ ] P2.2 Move `crates/gc_cli_driver/src/semantic_workspace.rs` from `phase-1 in-progress` to `phase-3 migrated` with GC-owned planning/edit graph logic as source of truth for agent workspace mutation workflows.
- [ ] P2.3 Move `crates/gc_patches/src/lib.rs` from `phase-1 in-progress` to `phase-3 migrated` so semantic patch construction/normalization is GC-owned in production execution paths.

### Current execution batch (2026-02-24)

- [x] P2.3.a Made selfhost `core/cli::validate-patch` authoritative in `gc_patches` production execution; removed silent Rust fallback on `"unknown :op"` validation failures.
- [x] P2.3.b Added frontend-aware patch validation API (`validate_patch_term_with_frontend`) and wired semantic workspace refactor-plan validation through the selected CoreForm frontend.
- [x] P2.3.c Added regression coverage proving poisoned selfhost `validate-patch` now hard-fails apply-patch instead of falling back to Rust acceptance.
- [x] P2.3.d Removed the Rust-only `validate_patch_term` entrypoint from `gc_patches`; all patch validation callsites now pass explicit frontend + limits.
- [x] P2.2.a Added semantic-edit selfhost poisoning regression (`cli_semantic_edit`) proving refactor-plan fails closed when selfhost `core/cli::validate-patch` rejects patch schema.
- Next for P2.3: migrate remaining Rust-owned refactor transforms (`rename-symbol`, `move-module`, `split-module`, `rewrite-*`, `migrate-contract-signature`) into selfhost `.gc` contracts for full phase-3 closure.

## Evidence Anchors

- `policies/source_decomposition_progress.toml`
- `docs/spec/GC_MODULE_BOUNDARIES_v0.1.md`
- `docs/spec/FULL_SELFHOST_CUTOVER_PROFILE_v0.1.md`
- `docs/spec/SELF_HOST_BOUNDARY.md`
- `docs/spec/TYPES.md`
- `crates/gc_cli_driver/src/cmd_registry.rs`
- `crates/gc_cli_driver/src/pkg_workspace_ops_build_artifacts.rs`
- `crates/gc_effects/src/policy_tests.rs`
- `.genesis/perf/agent_capability_gauntlet_report.json`
- `.genesis/perf/gpu_gfx_headroom_conformance_report.json`
