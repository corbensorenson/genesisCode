# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-25

Scope:
- Track only unresolved upgrades required for AI-first authoring reliability, selfhost robustness, and productization trust.
- Keep this file machine-syncable with `.genesis/perf/selfhost_readiness_report.json` and `feature_matrix.md`.
- Keep completed work out of this file (git history + perf artifacts are closure evidence).

Open checklist items: 0

## Critical Path

- [x] P2.1 Split oversized production Rust modules that exceed the 700-line maintainability target.
  Evidence this pass:
  - [x] `crates/gc_cli_driver/src/cmd_pkg.rs` split via `crates/gc_cli_driver/src/cmd_pkg/local_workspace_ops.rs` (261 + 550 lines).
  - [x] `crates/gc_opt/src/lib.rs` split via `crates/gc_opt/src/pure_egg.rs` (558 + 336 lines).
  - [x] `crates/gc_wasm/src/lib.rs` split via `crates/gc_wasm/src/runtime.rs` (415 + 430 lines).
  - [x] `crates/gc_opt/src/stage2_wasm/planner_helpers.rs` split via `crates/gc_opt/src/stage2_wasm/planner_helpers/collection_aliases.rs` (334 + 436 lines).
  - [x] `crates/gc_cli_driver/src/cli_args.rs` split via `crates/gc_cli_driver/src/cli_args/command_groups.rs` (559 + 359 lines).
  - [x] `crates/gc_opt/src/stage2_wasm/strings_bytes_lowering.rs` split via `crates/gc_opt/src/stage2_wasm/strings_bytes_scalar_lowering.rs` and `crates/gc_opt/src/stage2_wasm/strings_bytes_hex_lowering.rs` (302 + 385 + 214 lines).
  - [x] `crates/gc_cli_driver/src/cmd_pkg/frontend_dispatch/selfhost.rs` split via `crates/gc_cli_driver/src/cmd_pkg/frontend_dispatch/selfhost/init_add.rs` (696 + 173 lines).
  - [x] `crates/gc_obligations/src/obligation_gfx.rs` split via `crates/gc_obligations/src/obligation_gfx/helpers.rs` (616 + 308 lines).
  - [x] `crates/gc_prelude/src/selfhost_coreform_v1.rs` split via `crates/gc_prelude/src/selfhost_coreform_manifest.rs` (695 + 210 lines).
  - [x] `crates/gc_effects/src/runner_vcs_pkg_helpers/pkg_resolution.rs` split via `crates/gc_effects/src/runner_vcs_pkg_helpers/pkg_resolution/lock_validation.rs` (365 + 577 lines).
  - [x] `crates/gc_kernel/src/eval_prims.rs` split via `crates/gc_kernel/src/eval_prims/text_bytes.rs` (393 + 443 lines).
  - [x] `crates/gc_effects/src/runner_cap_gc_gpk_low.rs` split via `crates/gc_effects/src/runner_cap_gc_gpk_low/gpk_ops.rs` (381 + 559 lines).
  - [x] `crates/gc_effects/src/runner_cap_vcs_low/dispatch_patch_contract.rs` split via `crates/gc_effects/src/runner_cap_vcs_low/dispatch_patch_contract/merge_ops.rs` and `crates/gc_effects/src/runner_cap_vcs_low/dispatch_patch_contract/resolve_conflict.rs` (139 + 179 + 533 lines).
  - [x] `crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution.rs` split via `crates/gc_effects/src/runner_cap_pkg_low/dispatch_resolution/install_verify.rs` (675 + 315 lines).
  - [x] Validation: `cargo check -p gc_opt -p gc_wasm -p gc_cli_driver` passes.
  - [x] Validation: `cargo check -p gc_cli_driver -p gc_opt` passes.
  - [x] Validation: `cargo check -p gc_effects -p gc_obligations -p gc_prelude` passes.
  - [x] Validation: `cargo check -p gc_kernel -p gc_effects` passes.
  - [x] Validation: `cargo test -p gc_opt -p gc_wasm -p gc_cli_driver --lib` passes.
  - [x] Validation: `cargo test -p gc_opt -p gc_cli_driver --lib` passes.
  - [x] Validation: `cargo test -p gc_effects -p gc_obligations -p gc_prelude --lib` passes.
  - [x] Validation: `cargo test -p gc_kernel -p gc_effects --lib` passes.
  - [x] Validation: `bash scripts/check_source_decomposition_progress.sh` passes (`observed_max_lines=690`).
  - [x] Validation: `bash scripts/check_upgrade_plan_health.sh` passes (`elapsed_ms=200750`, `gate_count=55`).
  Remaining >700-line production files (0):
  - none
  Exit criteria:
  - All production modules are <=700 lines through behavior-preserving decomposition.
  - Cargo checks/tests remain green for touched crates.
  - Decomposition policy and health gates continue to pass.

## Evidence Anchors

- `.genesis/perf/selfhost_readiness_report.json`
- `.genesis/perf/full_selfhost_cutover_profile_report.json`
- `.genesis/perf/runtime_microbench_runtime_report.json`
- `.genesis/perf/hot_path_runtime_report.json`
- `.genesis/perf/production_cli_help_surface_report.json`
- `policies/source_decomposition_progress.toml`
