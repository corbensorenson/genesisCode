# GPU Compute Device Bridge v0.1

This spec defines the first-party in-repo device bridge path for `gpu/compute::*` host ops.

For compute-only runtime productization and CI gating that is independent of graphics
runtime lanes, see:

- `docs/spec/GPU_COMPUTE_RUNTIME_PROFILE_v0.1.md`

## Bridge Mode

- Command mode: `gc_runtime_bench --gpu-compute-bridge <op>`
- Request frame: `<len>\n<payload-term>`
- Response frame: `<len>\n<term>`
- Supported ops:
  - `gpu/compute::submit`
  - `gpu/compute::limits`
  - `gpu/compute::features`

## Backend Selection

Selection order in runtime microbench when
`GENESIS_GPU_COMPUTE_BACKEND_POLICY=dev-allow-fallback`:

1. `GENESIS_GPU_COMPUTE_DEVICE_BRIDGE_CMD` override (explicit external command).
2. First-party in-repo bridge (`gc_runtime_bench --gpu-compute-bridge`) when built with feature `device-bridge`.
3. Deterministic fallback bridge script.

When `GENESIS_GPU_COMPUTE_BACKEND_POLICY=require-device`, step (3) is disabled
and benchmark execution fails closed if no device-grade bridge is available.

Reported backend labels:

- `device-bridge`
- `deterministic-fallback`

## CI/Perf Policy

- GPU lane compiles runtime microbench with `GENESIS_RUNTIME_MICROBENCH_FEATURES=device-bridge`.
- GPU lane sets `GENESIS_GPU_COMPUTE_BACKEND_POLICY=require-device`.
- GPU lane enforces `GENESIS_RUNTIME_MICROBENCH_REQUIRED_GPU_BACKEND=device-bridge`.
- Dev/test lanes may keep deterministic fallback available by using
  `GENESIS_GPU_COMPUTE_BACKEND_POLICY=dev-allow-fallback`.

## Reference Capability Profiles

- `docs/policies/gpu_compute_bridge_device_caps_v0.1.toml`
- `docs/policies/gpu_compute_bridge_fallback_caps_v0.1.toml`
