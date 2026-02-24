# Feature Matrix Evidence Ledger v0.1

Machine-verifiable capability-to-evidence mapping generated from `feature_matrix.md`.

- Contract kind: `genesis/feature-matrix-evidence-v0.1`
- Capability entries: `19`
- Source matrix: `feature_matrix.md`

| Capability | Evidence Paths | Gate/Test Paths |
| --- | --- | --- |
| Pure deterministic kernel separated from effects | `docs/spec/CLI.md`<br>`docs/spec/SEALS_DISPATCH_REPLAY.md`<br>`docs/spec/DETERMINISM.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_no_user_panics.sh` |
| Canonical semantic IR + stable content hash identity | `docs/spec/CLI.md`<br>`docs/spec/COREFORM_CANON_HASH.md` | `scripts/check_upgrade_plan_health.sh` |
| Sealed unforgeable effect/error protocol | `docs/spec/CLI.md`<br>`docs/spec/SEALS_DISPATCH_REPLAY.md`<br>`docs/spec/DETERMINISM.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_no_user_panics.sh` |
| Deny-by-default capability policy runtime | `docs/spec/CLI.md` | `scripts/check_upgrade_plan_health.sh` |
| Deterministic effect logs + replay checks | `docs/spec/CLI.md`<br>`docs/spec/SEALS_DISPATCH_REPLAY.md`<br>`docs/spec/DETERMINISM.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_no_user_panics.sh` |
| Built-in semantic VCS (`commit/patch/refs`) | `docs/spec/CLI.md` | `scripts/check_upgrade_plan_health.sh` |
| Built-in package/project manager (`pkg`/`gcpm`) | `docs/spec/CLI.md`<br>`docs/spec/GCPM_BUNDLE_v0.1.md`<br>`docs/spec/GCPM_JSON_SCHEMAS_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_foundation_stdlib_conformance.sh` |
| Selfhost frontend default in production CLIs | `docs/spec/CLI.md`<br>`docs/spec/SELF_HOST_BOUNDARY.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_selfhost_boundary.sh`<br>`scripts/check_selfhost_artifact_fresh.sh` |
| Full cutover profile wired into default inner-loop health | `docs/spec/CLI.md` | `scripts/check_upgrade_plan_health.sh` |
| Selfhost guard robustness against stale local binaries | `docs/spec/CLI.md`<br>`docs/spec/SELF_HOST_BOUNDARY.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_selfhost_boundary.sh`<br>`scripts/check_selfhost_artifact_fresh.sh` |
| Agent workflow gauntlet (service/network/data/gfx/gpu/deploy/xr) | `docs/spec/CLI.md`<br>`docs/spec/GPU_GFX_BUNDLE_v0.1.md`<br>`docs/spec/GPU_COMPUTE_RUNTIME_PROFILE_v0.1.md`<br>`docs/spec/AGENT_AUTHORING_BUNDLE_v0.1.md`<br>`docs/spec/AGENT_CAPABILITY_GAUNTLET_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_gpu_compute_runtime_profile.sh`<br>`scripts/check_gpu_stack_decoupling.sh`<br>`scripts/check_gfx_runtime_profile.sh`<br>`scripts/check_agent_reference_workflows.sh` |
| Runtime skill-pack conformance breadth across required domains | `docs/spec/CLI.md` | `scripts/check_upgrade_plan_health.sh` |
| Deterministic concurrency/task replay surface | `docs/spec/CLI.md`<br>`docs/spec/SEALS_DISPATCH_REPLAY.md`<br>`docs/spec/DETERMINISM.md`<br>`docs/spec/CONCURRENCY_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_no_user_panics.sh`<br>`scripts/check_task_concurrency_stress.sh` |
| GPU compute capability independent of graphics surface | `docs/spec/CLI.md`<br>`docs/spec/GPU_GFX_BUNDLE_v0.1.md`<br>`docs/spec/GPU_COMPUTE_RUNTIME_PROFILE_v0.1.md`<br>`docs/spec/GPU_COMPUTE_BUNDLE_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_gpu_compute_runtime_profile.sh`<br>`scripts/check_gpu_stack_decoupling.sh`<br>`scripts/check_gfx_runtime_profile.sh` |
| Graphics/window/input/audio capability families | `docs/spec/CLI.md`<br>`docs/spec/GPU_GFX_BUNDLE_v0.1.md`<br>`docs/spec/GPU_COMPUTE_RUNTIME_PROFILE_v0.1.md`<br>`docs/spec/GFX_RUNTIME_BUNDLE_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_gpu_compute_runtime_profile.sh`<br>`scripts/check_gpu_stack_decoupling.sh`<br>`scripts/check_gfx_runtime_profile.sh` |
| Strict GPU/XR runtime evidence as default productization lane | `docs/spec/CLI.md`<br>`docs/spec/GPU_GFX_BUNDLE_v0.1.md`<br>`docs/spec/GPU_COMPUTE_RUNTIME_PROFILE_v0.1.md`<br>`docs/spec/XR_HOST_RUNTIME_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_gpu_compute_runtime_profile.sh`<br>`scripts/check_gpu_stack_decoupling.sh`<br>`scripts/check_gfx_runtime_profile.sh`<br>`scripts/check_webxr_browser_conformance_lane.sh` |
| Deployment target pipeline in core toolchain | `docs/spec/CLI.md`<br>`docs/spec/GCPM_JSON_SCHEMAS_v0.1.md`<br>`docs/spec/GCPM_WORKFLOW_REPORTS_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`crates/gc_cli/tests/cli_pkg_workspace.rs`<br>`examples/agent_deploy_bundle_workflow/workflow.sh` |
| Reachability-based artifact GC (`refs`/locks/pins) | `docs/spec/CLI.md`<br>`docs/spec/GCPM_BUNDLE_v0.1.md`<br>`docs/spec/GCPM_JSON_SCHEMAS_v0.1.md`<br>`docs/spec/SELF_HOST_BOUNDARY.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_foundation_stdlib_conformance.sh`<br>`scripts/check_selfhost_boundary.sh`<br>`scripts/check_selfhost_artifact_fresh.sh` |
| Production module decomposition for AI maintainability | `docs/spec/CLI.md` | `scripts/check_upgrade_plan_health.sh` |
