# Concurrency + GPU SLO Contract v0.1

Status: normative CI contract for deterministic concurrency and GPU/compute throughput gates.

## Scope

This contract defines production SLO checks derived from runtime microbench evidence:

- `task_scheduler` throughput budget from `metrics.task_runner_ms`
- `gpu_compute_bridge` throughput budget from `metrics.bridge_runner_ms`
- `gpu_compute_submit` throughput budget from `metrics.gpu_compute_submit_ms`

Both budgets are measured by `gc_runtime_bench` and enforced in CI by
`scripts/check_runtime_microbench_budgets.sh`.

## Input Evidence

Source report (required):
- `.genesis/perf/runtime_microbench_metrics.json`
- kind: `genesis/runtime-microbench-v0.1`

Required fields:
- `metrics.bridge_runner_ms`
- `metrics.gpu_compute_submit_ms`
- `metrics.task_runner_ms`
- `budgets.bridge_runner_ms`
- `budgets.gpu_compute_submit_ms`
- `budgets.task_runner_ms`
- `gpu_compute_backend` (`"deterministic-fallback"` or `"device-runtime"`)
- `gpu_compute_backend_policy` (`"dev-allow-fallback"` or `"require-device"`)

## Output Artifact

CI must emit:
- `.genesis/perf/concurrency_gpu_slo_report.json`
- kind: `genesis/concurrency-gpu-slo-v0.1`

Schema:

```json
{
  "kind": "genesis/concurrency-gpu-slo-v0.1",
  "source_report": ".genesis/perf/runtime_microbench_metrics.json",
  "gpu_compute_backend": "deterministic-fallback",
  "gpu_compute_backend_policy": "dev-allow-fallback",
  "ci_enforced": true,
  "slo": {
    "gpu_compute_bridge": {
      "metric": "bridge_runner_ms",
      "observed_ms": 0,
      "budget_ms": 0,
      "ok": true
    },
    "gpu_compute_submit": {
      "metric": "gpu_compute_submit_ms",
      "observed_ms": 0,
      "budget_ms": 0,
      "ok": true
    },
    "task_scheduler": {
      "metric": "task_runner_ms",
      "observed_ms": 0,
      "budget_ms": 0,
      "ok": true
    }
  },
  "ok": true
}
```

## Failure Policy

Fail closed if:
- required metrics/budgets are missing
- observed latency exceeds budget for either SLO

Failure behavior:
- script exits non-zero
- CI lane fails

## CI Integration

`.github/workflows/ci.yml` runs `scripts/check_runtime_microbench_budgets.sh` on
`standard` and `full` profiles and uploads `.genesis/perf/*.json` as artifacts.

## Device-Backed Compute Path

`gc_runtime_bench` now includes a dedicated GPU compute submit benchmark path:

- default mode: deterministic fallback bridge (`gpu_compute_backend = deterministic-fallback`)
- device mode: set `GENESIS_GPU_COMPUTE_DEVICE_BRIDGE_CMD=/abs/path/to/bridge` to benchmark a
  device-backed bridge path (`gpu_compute_backend = device-runtime`)

This keeps the bridge-overhead metric (`bridge_runner_ms`) and compute-submit metric
(`gpu_compute_submit_ms`) independently visible in SLO reports.

Perf-critical lanes must set `GENESIS_GPU_COMPUTE_BACKEND_POLICY=require-device`
so fallback is never accepted implicitly.

For compute-only runtime profile gating that explicitly excludes non-compute lanes,
see `docs/spec/GPU_COMPUTE_RUNTIME_PROFILE_v0.1.md`.
