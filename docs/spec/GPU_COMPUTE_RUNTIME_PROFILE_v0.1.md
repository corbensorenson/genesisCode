# GPU Compute Runtime Profile v0.1

Status: normative contract for compute-only runtime profiling and CI gating.

## Purpose

Define a dedicated runtime profile for `gpu/compute::*` that is independent from
graphics (`gfx/window::*`, `gfx/input::*`, `gfx/audio::*`, `gfx/gpu::*`) execution lanes.

This profile is compute-product focused and is used to ensure:

- `gpu/compute::submit` throughput remains within budget.
- compute benchmarking can run without requiring graphics subsystems.
- CI has a compute-specific performance gate that is not coupled to gfx pathways.

## Entry Point

- Script: `scripts/check_gpu_compute_runtime_profile.sh`
- Primary artifact: `.genesis/perf/gpu_compute_runtime_profile.json`
- Guard artifact: `.genesis/perf/gpu_compute_runtime_profile_guard.json`

## Runtime Invocation

The profile executes `gc_runtime_bench` in compute-only mode:

```bash
cargo run -p gc_runtime_bench -- --mode compute-only --out .genesis/perf/gpu_compute_runtime_profile.json
```

Compute-only mode requirements:

- `bench_mode` must be `"compute-only"`.
- `metrics.gpu_compute_submit_ms` is measured and budget-enforced.
- Non-compute metrics must remain zero:
  - `eval_ms`
  - `runner_ms`
  - `bridge_runner_ms`
  - `task_runner_ms`
  - `patch_apply_ms`
  - `store_cycle_ms`
  - `sync_pull_ms`
- Device-runtime executions additionally emit `gpu_compute_adapter` in the runtime report.

## Policy Inputs

Optional environment knobs:

- `GENESIS_GPU_COMPUTE_RUNTIME_PROFILE_REQUIRED_BACKEND`
  - If set, backend must match (`device-runtime` or `deterministic-fallback`).
  - Legacy alias `device-bridge` is accepted and normalized to `device-runtime`.
- `GENESIS_RUNTIME_MICROBENCH_FEATURES`
  - For device-backed runs use `device-bridge` (feature name); emitted backend label remains `device-runtime`.
- `GENESIS_GPU_COMPUTE_BACKEND_POLICY`
  - `require-device` for perf-critical/release lanes (fails closed if no device backend exists).
  - `dev-allow-fallback` for explicit dev/test fallback mode.
- `GENESIS_PERF_CARGO_PROFILE`
  - Build profile for benchmark execution.

Per-op `caps.toml` knobs for first-party runtime lanes:

- `gpu_backend = "device-runtime"` selects in-repo device-backed execution for
  `gpu/compute::*` and `gfx/gpu::*` submit/introspection ops.
- `gpu_backend = "device-runtime-full"` requests device backend routing for canonical
  GPU lifecycle operations (create/write/read/destroy + submit/introspection).
- `gpu_backend_policy = "require-device|allow-fallback"` defines fail-closed vs fail-open
  behavior when a device backend is unavailable.

## CI Integration

CI enforces this profile in standard/full lanes before strict selfhost suites:

- `.github/workflows/ci.yml` runs `bash scripts/check_gpu_compute_runtime_profile.sh`.
- `.github/workflows/ci.yml` also runs `bash scripts/check_gpu_compute_device_conformance.sh`
  in the dedicated `gpu_device_microbench` self-hosted GPU lane to enforce
  `require-device` backend policy and persist adapter-specific artifacts.
- `scripts/check_upgrade_plan_health.sh` includes the same guard for prepush/release profiles.
  Optional device conformance in health profiles is enabled with
  `GENESIS_HEALTH_REQUIRE_GPU_DEVICE_CONFORMANCE=1`.

## Reference Workload

Compute-first selfhost reference workflow:

- `examples/agent_gpu_compute_workflow/`
  - package test + run/replay determinism
  - task scheduler + `gpu/compute::submit`
  - no `gfx/gpu::*` dependency

This ensures agent-oriented compute workflows remain validated even when graphics
subsystems are unavailable.
