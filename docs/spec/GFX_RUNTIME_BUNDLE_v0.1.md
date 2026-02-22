# GFX Runtime Bundle v0.1

Canonical bundle for graphics runtime and rendering-first capability contracts.

## Included Specs

- `docs/spec/GFX_CAPS.md`
- `docs/spec/XR_HOST_RUNTIME_v0.1.md`
- `docs/spec/BROWSER_HOST_RUNTIME_v0.1.md`
- `docs/spec/GPU_COMPUTE_BUNDLE_v0.1.md` (cross-over boundary reference)

## Cross-Over Points

Graphics and compute remain independently verifiable:

- graphics-only conformance lane:
  - `scripts/check_gfx_runtime_profile.sh`
- compute-only conformance lane:
  - `scripts/check_gpu_compute_runtime_profile.sh`

Shared primitives are restricted to explicit interop surfaces:

- `gfx/gpu::*` resource/render pipeline ops
- `gpu/compute::*` compute submission ops

Cross-over must be intentional and policy-gated; graphics profiles should not depend on
compute-only conformance outcomes unless they explicitly opt into shared compute paths.
