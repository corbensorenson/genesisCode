# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-24

Scope:
- List only unresolved blockers/risk-reduction work for selfhost closure and AI-first productization.
- Remove completed items; rely on git history and perf artifacts for closure evidence.
- Machine-readable source of unresolved IDs: `.genesis/perf/selfhost_readiness_report.json`.

Open checklist items: 4

## Selfhost Closure (Rust Ownership Still In Critical Path)

- [ ] P2.1 Move `crates/gc_obligations/src/obligation_exec.rs` from `phase-1 in-progress` to `phase-3 migrated` by routing production obligation orchestration through `.gc` ownership and archiving Rust semantic sidecars to parity-only paths.
- [ ] P2.2 Move `crates/gc_cli_driver/src/semantic_workspace.rs` from `phase-1 in-progress` to `phase-3 migrated` with GC-owned planning/edit graph logic as source of truth for agent workspace mutation workflows.
- [ ] P2.3 Move `crates/gc_patches/src/lib.rs` from `phase-1 in-progress` to `phase-3 migrated` so semantic patch construction/normalization is GC-owned in production execution paths.

## Iteration Throughput

- [ ] P3.1 Reduce `prepush-standard` and strict-profile iteration latency via deterministic shard scheduling/caching policy so full-quality loops are routinely single-digit minutes on warm cache.

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
