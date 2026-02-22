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
