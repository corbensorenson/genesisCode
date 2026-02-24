# GenesisCode `.gc` Module Boundaries v0.1

This document defines maintainability boundaries for source-of-truth `.gc` modules used by AI agents.

## Scope

Applies to:

- all `.gc` paths resolved from policy `gc_source_roots` (directories scanned recursively):
  - `/Users/corbensorenson/Documents/genesisCode/prelude/modules`
  - `/Users/corbensorenson/Documents/genesisCode/selfhost`
  - `/Users/corbensorenson/Documents/genesisCode/prelude/prelude.gc`

Generated artifacts are excluded:

- policy allowlist `gc_generated_exclude_paths` (currently `/Users/corbensorenson/Documents/genesisCode/prelude/prelude.gc`)

Notes:

- `/Users/corbensorenson/Documents/genesisCode/selfhost/toolchain.gc` remains a generated assembly artifact, but is now emitted in compact CoreForm form and stays within enforced `.gc` line budgets (no policy carve-out).
- `gc_prelude` bootstrap now assembles embedded prelude source from `prelude/modules/manifest.toml` at build time; runtime no longer consumes `prelude/prelude.gc` as its source of truth.

## Boundary Rules

- Keep modules domain-focused and composable:
  - `prelude/modules/00_*` for core data/effect/protocol helpers
  - `prelude/modules/10_*` for gfx/compute wrappers and runtime traces
  - `prelude/modules/20_*` for editor/tasking surfaces
    - split into focused units (host-ops, vcs, ast, plugin, action orchestration) to avoid monolithic editor modules
  - `prelude/modules/30_*` for reusable high-level domain kits (service orchestration, data pipelines, network workflows, game-loop scaffolding, XR runtime orchestration, media asset pipelines)
  - `selfhost/cli_*` for CLI/runtime orchestration
  - `selfhost/{parse,canon,printer,hash}` for frontend core
  - `selfhost/stage1_*` and patch schema modules for optimization/rewrites
- Prefer adding a new module over extending an existing module past budget.
- Expose stable, small top-level entrypoints and keep helper internals local to each module.

## Budget Enforcement

`.gc` source budgets are enforced by:

- `/Users/corbensorenson/Documents/genesisCode/scripts/check_gc_source_size_budget.sh`
- policy file: `/Users/corbensorenson/Documents/genesisCode/policies/source_size_budget.toml`

Current policy tracks:

- `gc_max_lines`
- `gc_target_lines`
- generated-artifact exclusions
- explicit target-debt allowlist (`gc_target_exclude_paths`)

## AI-First Rationale

- Smaller, domain-scoped modules improve agent planning and reduce edit conflicts.
- Stable boundaries reduce prompt context size and increase rewrite reliability.
- Budget gates prevent silent drift into monolithic files that are hard for both agents and humans to maintain.

## Selfhost Migration Plan (High-Churn Rust -> GC)

Goal: reduce high-churn Rust production ownership by moving behavior into GC-authored modules where parity is proven.

Phase model:

- `phase-0`: extraction planning complete, no behavior moved yet.
- `phase-1`: shared contract/data model moved to GC modules with parity harness checks.
- `phase-2`: runtime dispatch moved to GC-first path, Rust path retained as parity-only sidecar.
- `phase-3`: Rust implementation removed from production path; historical logic archived under `old_bootstrap/` or parity-only test harnesses.

| Rust module | Target GC module(s) | Parity evidence gate | Phase | Status |
|---|---|---|---|---|
| `crates/gc_cli_driver/src/cmd_selfhost.rs` | `selfhost/toolchain.gc`, `selfhost/toolchain_manifest.gc` | `bash scripts/check_selfhost_readiness_scorecard.sh` | `phase-2` | `migrated` |
| `crates/gc_cli_driver/src/pkg_workspace_ops.rs` | `prelude/modules/31_data_pipeline.gc` | `bash scripts/check_agent_reference_workflows.sh` | `phase-2` | `migrated` |
| `crates/gc_obligations/src/obligation_exec.rs` | `prelude/modules/30_service_orchestration.gc`, `prelude/modules/31_data_pipeline.gc` | `bash scripts/check_agent_generative_workloads.sh` | `phase-3` | `migrated` |
| `crates/gc_gfx/src/lib.rs` | `prelude/modules/33_game_loop.gc`, `prelude/modules/34_xr_workflow.gc` | `bash scripts/check_gfx_runtime_profile.sh` | `phase-2` | `migrated` |
| `crates/gc_prelude/src/prelude.rs` | `prelude/modules/manifest.toml` | `bash scripts/check_prelude_capability_coverage.sh` | `phase-2` | `migrated` |
| `crates/gc_types/src/infer.rs` | `prelude/modules/36_semantic_workspace.gc`, `selfhost/toolchain.gc` | `cargo test -p gc_types --lib --quiet` | `phase-3` | `migrated` |
| `crates/gc_cli_driver/src/semantic_workspace.rs` | `prelude/modules/36_semantic_workspace.gc`, `prelude/modules/32_network_workflow.gc` | `bash scripts/check_agent_reference_workflows.sh` | `phase-3` | `migrated` |
| `crates/gc_patches/src/lib.rs` | `prelude/modules/32_network_workflow.gc` | `bash scripts/check_task_concurrency_stress.sh` | `phase-3` | `migrated` |
| `crates/gc_types/src/lib.rs` | `prelude/modules/36_semantic_workspace.gc`, `selfhost/toolchain.gc` | `bash scripts/check_write_genesiscode_skill_conformance.sh` | `phase-3` | `migrated` |
| `crates/gc_kernel/src/eval.rs` | `selfhost/toolchain.gc`, `prelude/modules/00_core_media.gc` | `bash scripts/check_kernel_tcb_contract.sh` | `phase-3` | `migrated` |
| `crates/gc_cli_driver/src/cmd_vcs.rs` | `prelude/modules/32_network_workflow.gc` | `bash scripts/check_vcs_selfhost_contract.sh` | `phase-3` | `migrated` |
| `crates/gc_effects/src/runner_host_bridge.rs` | `prelude/modules/10_browser_host.gc`, `prelude/modules/10_xr_host.gc` | `bash scripts/check_host_bridge_fault_injection.sh` | `phase-2` | `migrated` |
| `crates/gc_registry/src/registry/client_impl/ping_and_store.rs` | `prelude/modules/32_network_workflow.gc` | `cargo test -p gc_registry --quiet` | `phase-2` | `migrated` |
| `crates/gc_vcs/src/policy.rs` | `prelude/modules/32_network_workflow.gc` | `cargo test -p gc_vcs --quiet` | `phase-3` | `migrated` |
| `crates/gc_opt/src/stage2_wasm/collections_lowering.rs` | `selfhost/toolchain.gc`, `prelude/modules/31_data_pipeline.gc` | `cargo test -p gc_opt --lib --quiet` | `phase-2` | `migrated` |
| `crates/gc_effects/src/runner_xr_host/advanced.rs` | `prelude/modules/10_xr_host.gc`, `prelude/modules/34_xr_workflow.gc` | `cargo test -p gc_effects --lib --quiet` | `phase-2` | `migrated` |
| `crates/gc_effects/src/runner_capability_dispatch/net.rs` | `prelude/modules/32_network_workflow.gc`, `prelude/modules/10_browser_host.gc` | `cargo test -p gc_effects --lib --quiet` | `phase-2` | `migrated` |
| `crates/gc_patches/src/patch_apply.rs` | `prelude/modules/36_semantic_workspace.gc`, `prelude/modules/31_data_pipeline.gc` | `cargo test -p gc_patches --quiet` | `phase-2` | `migrated` |
| `crates/gc_kernel/src/compiled.rs` | `selfhost/toolchain.gc`, `prelude/modules/00_core_media.gc` | `cargo test -p gc_kernel --quiet` | `phase-2` | `migrated` |
| `crates/gc_cli_driver/src/pkg_assurance_ops.rs` | `prelude/modules/31_data_pipeline.gc`, `prelude/modules/36_semantic_workspace.gc` | `cargo test -p gc_cli_driver --quiet` | `phase-2` | `migrated` |

Exit criteria:

1. Target GC module path is live in production authoring flow.
2. Parity evidence gate is green in strict profile lanes.
3. Rust path is no longer on production critical path (parity-only or archived).
4. `policies/source_decomposition_progress.toml` is updated in the same change.
