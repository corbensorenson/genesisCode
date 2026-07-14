> Bundle Entry: `docs/spec/GPU_GFX_BUNDLE_v0.1.md`
> Legacy Split Doc: Prefer the bundle entrypoint for agent retrieval; this file retains detailed, topic-local semantics.

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

- Read-only check: `scripts/check_gpu_compute_runtime_profile.sh`
- Explicit producer: `scripts/update_gpu_compute_runtime_profile_report.sh`
- Renderer with caller-owned outputs: `scripts/render_gpu_compute_runtime_profile_report.sh`
- Optional primary artifact: `.genesis/perf/gpu_compute_runtime_profile.json`
- Optional guard artifact: `.genesis/perf/gpu_compute_runtime_profile_guard.json`
- Optional timing evidence: `.genesis/perf/gpu_compute_runtime_profile_runtime_report.json`
  and `.genesis/perf/gpu_compute_runtime_profile_runtime_history.jsonl`

The check always renders into a private temporary directory and treats retained
history as input-only. Output environment overrides are accepted only by the
explicit producer. CI lanes that upload E0 observations invoke the producer;
ordinary validation and health profiles invoke the read-only check.

## Runtime Invocation

The profile executes `gc_runtime_bench` in compute-only mode:

```bash
bash scripts/check_gpu_compute_runtime_profile.sh

# Retain a local E0 report set explicitly.
bash scripts/update_gpu_compute_runtime_profile_report.sh
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
- `GENESIS_GPU_BACKEND_POLICY_DEFAULT`
  - global runtime default for per-op GPU backend fallback when `caps.toml` does not set
    `gpu_backend_policy`.
  - strict/default high-confidence health lanes (`agent-inner-loop`, `prepush-standard`,
    `release-full`, `full-selfhost-cutover`) set this to `require-device`.
  - fallback behavior is opt-in through explicit `GENESIS_AGENT_GPU_PROFILE=agent-gpu-fallback`.
- `GENESIS_AGENT_GPU_PROFILE`
  - explicit automation contract selection:
    - `agent-gpu-strict` -> fail-closed (`require-device`)
    - `agent-gpu-fallback` -> explicit fallback (`allow-fallback` / `dev-allow-fallback`)
  - enforced in automation contexts by `scripts/check_agent_gpu_profile_contract.sh`.
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

- `.github/workflows/ci.yml` runs
  `bash scripts/update_gpu_compute_runtime_profile_report.sh` because that lane uploads the
  resulting E0 metrics, guard, report, and history files. Read-only aggregate health profiles
  use `bash scripts/check_gpu_compute_runtime_profile.sh`.
- `.github/workflows/ci.yml` also runs
  `bash scripts/update_gpu_compute_device_conformance_report.sh`
  in two independent lanes:
  - `gpu_device_microbench` (`self-hosted, linux, x64, gpu`)
  - `gpu_device_microbench_deterministic` (`ubuntu-latest` deterministic runtime command)
  Both lanes enforce `require-device` backend policy and persist adapter-specific artifacts.
- `.github/workflows/ci.yml` defines an expanded real-hardware matrix gate (enabled with
  `GENESIS_GPU_MATRIX_ENABLED=1`) with explicit lane metadata:
  - `gpu_device_microbench_nvidia_linux`
  - `gpu_device_microbench_amd_linux`
  - `gpu_device_microbench_intel_windows`
  - `gpu_device_microbench_apple_macos`
  Each lane emits adapter-suffixed retention artifacts plus lane-scoped summary reports with
  `lane_id`, `gpu_vendor`, and `os_family`. Each summary preserves the stable four-key
  `artifacts` contract and carries runtime report/history paths in the separate,
  backward-compatible `timing_artifacts` map.
- `.github/workflows/ci.yml` runs
  `bash scripts/update_gpu_device_conformance_lane_parity_report.sh` in
  `gpu_device_conformance_release_gate`; it validates downloaded lane contracts and retains
  the uploaded parity E0 artifact. Local validation uses the paired read-only check.
- `.github/workflows/ci.yml` runs
  `bash scripts/update_gpu_device_conformance_matrix_report.sh` in
  `gpu_device_conformance_matrix_gate` to enforce representative NVIDIA/AMD/Intel +
  Linux/macOS/Windows lane coverage and retain the matrix E0 artifact. Local validation uses
  `scripts/check_gpu_device_conformance_matrix.sh`.
- `scripts/check_upgrade_plan_health.sh` includes the same guard for prepush/release profiles.
  `release-full` requires device conformance by default; `dev-fast`/`prepush-standard`
  opt in with `GENESIS_HEALTH_REQUIRE_GPU_DEVICE_CONFORMANCE=1`.

## Reference Workload

Compute-first selfhost reference workflow:

- `examples/agent_gpu_compute_workflow/`
  - package test + run/replay determinism
  - task scheduler + `gpu/compute::submit`
  - no `gfx/gpu::*` dependency

This ensures agent-oriented compute workflows remain validated even when graphics
subsystems are unavailable.
