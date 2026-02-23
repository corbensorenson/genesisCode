# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-23

Scope:
- List only unresolved blockers/risk-reduction work for selfhost closure and AI-first productization.
- Remove completed items; rely on git history and perf artifacts for closure evidence.
- Machine-readable source of unresolved IDs: `.genesis/perf/selfhost_readiness_report.json`.

Open checklist items: 14

## Selfhost Closure (Rust Ownership Still In Critical Path)

- [ ] P2.1 Move `crates/gc_obligations/src/obligation_exec.rs` from `phase-1 in-progress` to `phase-3 migrated` by routing production obligation orchestration through `.gc` ownership and archiving Rust semantic sidecars to parity-only paths.
- [ ] P2.2 Move `crates/gc_cli_driver/src/semantic_workspace.rs` from `phase-1 in-progress` to `phase-3 migrated` with GC-owned planning/edit graph logic as source of truth for agent workspace mutation workflows.
- [ ] P2.3 Move `crates/gc_patches/src/lib.rs` from `phase-1 in-progress` to `phase-3 migrated` so semantic patch construction/normalization is GC-owned in production execution paths.
- [ ] P2.4 Move `crates/gc_kernel/src/eval.rs` migration row from `phase-1 in-progress` to explicit closure milestone (bounded permanent TCB contract) with a narrower Rust semantic surface and documented non-growth policy.
- [ ] P2.5 Move `crates/gc_cli_driver/src/cmd_vcs.rs` from `phase-1 in-progress` to `phase-3 migrated` so VCS high-level orchestration is GC-authored and Rust remains transport/adapter only.

## Compatibility/Fallback Debt

- [ ] P2.6 Remove production acceptance of legacy compatibility schemas/aliases (legacy high-level op tables, legacy payload aliases) behind strict profile defaults; keep only explicit parity-harness support where required.
- [ ] P2.7 Enforce release-profile fail-closed GPU backend policy (`require-device`) in agent gauntlets; keep deterministic fallback lanes as explicit dev/test-only profiles.
- [ ] P2.8 Replace target launcher shell stubs (`launch_*.sh`) with target-native execution/package adapters and deterministic verification hooks for each deployment target.
- [ ] P2.9 Add a WASI-compatible registry serving path (or equivalent selfhost registry contract) so registry hosting is not native-binary-only.

## Agent-First Capability Depth

- [ ] P2.10 Expand type/effect checking beyond the current gradual subset to improve agent reliability on large generated codebases (stronger row/effect inference and richer declared-shape validation).
- [ ] P2.11 Add deterministic, machine-checkable API evolution contracts for high-churn host surfaces (GPU/XR/editor/network/plugin) so agents can safely upgrade across versions without manual schema forensics.
- [ ] P2.12 Promote GC-native project operations (`pkg build/run/test/trace/qualify`) to a stable capability contract pack for autonomous agents (versioned operation contracts + deterministic failure taxonomy).

## Iteration Throughput

- [ ] P3.1 Reduce `prepush-standard` and strict-profile iteration latency via deterministic shard scheduling/caching policy so full-quality loops are routinely single-digit minutes on warm cache.
- [ ] P3.2 Split remaining high-churn Rust files (`crates/gc_kernel/src/eval.rs`, `crates/gc_cli_driver/src/cmd_vcs.rs`, `crates/gc_effects/src/runner_host_bridge.rs`) into smaller modules with clearer ownership boundaries for agent edit locality.

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
