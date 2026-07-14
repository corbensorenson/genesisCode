# GPU Compute Bundle v0.1

Canonical bundle for compute-first GPU capability contracts.

## Included Specs

- `docs/spec/GPU_COMPUTE_RUNTIME_PROFILE_v0.1.md`
- `docs/spec/CONCURRENCY_GPU_SLO_v0.1.md`
- `docs/spec/CAPS_TOML.md`
- `docs/spec/GFX_RUNTIME_BUNDLE_v0.1.md` (cross-over boundary reference)

## Cross-Over Points

Compute and graphics stay decoupled at the high-level policy lane:

- compute-only conformance lane:
  - `scripts/check_gpu_compute_runtime_profile.sh`
- graphics-only conformance lane:
  - `scripts/check_gfx_runtime_profile.sh`

Shared primitives are explicit and limited:

- shared backend family: `gpu/compute::*` and `gfx/gpu::*` submission/resource interop
- shared policy keys in `caps.toml` for bridge/device backend contract enforcement

When a graphics workflow needs compute interop, it must cross via these shared primitives
rather than collapsing into the compute lane policy.

## Productization Kits (Non-Gfx + XR)

Canonical authoring assets are intentionally consolidated into existing distribution docs to
avoid markdown sprawl:

- non-graphics GPU data/simulation recipe:
  - `docs/skill_pack/write_genesiscode_v1/recipes/gpu_compute_workflow.md`
  - manifest id: `gpu_data_simulation_workflow`
  - workflow: `examples/agent_compute_workflow/workflow.sh`
  - heavy-compute variant workflow: `examples/agent_gpu_compute_workflow/workflow.sh`
- XR deploy/test recipe:
  - `docs/skill_pack/write_genesiscode_v1/recipes/xr_workflow.md`
  - manifest id: `xr_deploy_test_workflow`
  - workflow: `scripts/check_gpu_xr_productization_kits.sh`
  - runtime workflow: `examples/agent_xr_runtime_workflow/workflow.sh`
  - browser conformance workflow: `scripts/check_webxr_browser_conformance.sh`

Determinism enforcement for combined non-gfx GPU + XR lanes:

- `scripts/check_gpu_xr_productization_kits.sh`

The check renders its report into a private temporary path. It consumes declared gauntlet
and WebXR reports, may render missing prerequisites privately only when
`GENESIS_GPU_XR_PRODUCTIZATION_AUTO_RUN_GAUNTLET=1`, and never retains them. Use
`scripts/update_gpu_xr_productization_kits_report.sh` to retain the productization report;
with auto-run enabled, that explicit producer may also invoke the paired gauntlet and WebXR
producers. Missing prerequisites otherwise fail with the exact producer command.

GPU/GFX headroom follows the same lifecycle:

- read-only validation: `scripts/check_gpu_gfx_headroom_conformance.sh`
- explicit report/history producer: `scripts/update_gpu_gfx_headroom_conformance_report.sh`
- retained report and history are input-only to the check through
  `GENESIS_GPU_GFX_HEADROOM_REPORT_INPUT` and
  `GENESIS_GPU_GFX_HEADROOM_HISTORY_INPUT`.
