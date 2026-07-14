# Concurrency + GPU SLO Contract v0.1

Status: normative CI contract for deterministic concurrency and GPU/compute throughput gates.

## Scope

This contract defines production SLO checks derived from runtime microbench evidence:

- `task_scheduler` throughput budget from `metrics.task_runner_ms`
- `gpu_compute_bridge` throughput budget from `metrics.bridge_runner_ms`
- `gpu_compute_submit` throughput budget from `metrics.gpu_compute_submit_ms`

Both budgets are measured by `gc_runtime_bench`. Read-only validation uses
`scripts/check_runtime_microbench_budgets.sh`; CI lanes that retain and upload
the E0 report set use `scripts/update_runtime_microbench_budgets_report.sh`.

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
- `gpu_compute_adapter` (required in device-runtime conformance lanes)

## Output Artifact

The explicit producer emits:
- `.genesis/perf/concurrency_gpu_slo_report.json`
- kind: `genesis/concurrency-gpu-slo-v0.1`

It also emits the source metrics plus
`.genesis/perf/runtime_microbench_runtime_report.json` and the one-row-per-run
`.genesis/perf/runtime_microbench_runtime_history.jsonl`. The read-only check
uses private temporary outputs and never appends retained history.

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

`.github/workflows/ci.yml` runs `scripts/update_runtime_microbench_budgets_report.sh` on
`standard` and `full` profiles, and runs
`scripts/update_gpu_compute_device_conformance_report.sh` on both GPU conformance lanes
(`gpu_device_microbench` + `gpu_device_microbench_deterministic`) to produce
adapter-scoped conformance artifacts. Health and local validation surfaces use the paired
read-only checks.
Release profile runs additionally retain validated lane parity with
`scripts/update_gpu_device_conformance_lane_parity_report.sh`; read-only local and aggregate
validation use the paired check or its explicit renderer with temporary output.
`scripts/check_upgrade_plan_health.sh --profile release-full` also requires device
conformance by default (fail-closed for release/full profile health gates).

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
