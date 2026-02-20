#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

OUT="${GENESIS_RUNTIME_MICROBENCH_OUT:-.genesis/perf/runtime_microbench_metrics.json}"
SLO_OUT="${GENESIS_CONCURRENCY_GPU_SLO_OUT:-.genesis/perf/concurrency_gpu_slo_report.json}"

echo "runtime-microbench: running benchmark suite"
cargo run -p gc_runtime_bench -- --out "$OUT"

echo "runtime-microbench: metrics"
cat "$OUT"

python3 - "$OUT" "$SLO_OUT" <<'PY'
import json
import pathlib
import sys

metrics_path = pathlib.Path(sys.argv[1])
slo_path = pathlib.Path(sys.argv[2])

doc = json.loads(metrics_path.read_text(encoding="utf-8"))
metrics = doc.get("metrics")
budgets = doc.get("budgets")

if not isinstance(metrics, dict) or not isinstance(budgets, dict):
    raise SystemExit("runtime-microbench: missing metrics/budgets map")

required = [
    "bridge_runner_ms",
    "gpu_compute_submit_ms",
    "task_runner_ms",
]
for key in required:
    if key not in metrics:
        raise SystemExit(f"runtime-microbench: missing metrics.{key}")
    if key not in budgets:
        raise SystemExit(f"runtime-microbench: missing budgets.{key}")

bridge_ms = int(metrics["bridge_runner_ms"])
bridge_budget = int(budgets["bridge_runner_ms"])
gpu_compute_submit_ms = int(metrics["gpu_compute_submit_ms"])
gpu_compute_submit_budget = int(budgets["gpu_compute_submit_ms"])
task_ms = int(metrics["task_runner_ms"])
task_budget = int(budgets["task_runner_ms"])

bridge_ok = bridge_ms <= bridge_budget
gpu_compute_submit_ok = gpu_compute_submit_ms <= gpu_compute_submit_budget
task_ok = task_ms <= task_budget
ok = bridge_ok and gpu_compute_submit_ok and task_ok

slo = {
    "kind": "genesis/concurrency-gpu-slo-v0.1",
    "source_report": str(metrics_path),
    "gpu_compute_backend": doc.get("gpu_compute_backend", "unknown"),
    "ci_enforced": True,
    "slo": {
        "gpu_compute_bridge": {
            "metric": "bridge_runner_ms",
            "observed_ms": bridge_ms,
            "budget_ms": bridge_budget,
            "ok": bridge_ok,
        },
        "gpu_compute_submit": {
            "metric": "gpu_compute_submit_ms",
            "observed_ms": gpu_compute_submit_ms,
            "budget_ms": gpu_compute_submit_budget,
            "ok": gpu_compute_submit_ok,
        },
        "task_scheduler": {
            "metric": "task_runner_ms",
            "observed_ms": task_ms,
            "budget_ms": task_budget,
            "ok": task_ok,
        },
    },
    "ok": ok,
}

slo_path.parent.mkdir(parents=True, exist_ok=True)
slo_path.write_text(json.dumps(slo, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"runtime-microbench: wrote concurrency/gpu slo report {slo_path}")

if not ok:
    raise SystemExit(
        "runtime-microbench: concurrency/gpu slo failure "
        f"(bridge={bridge_ms}/{bridge_budget}, gpu_compute_submit={gpu_compute_submit_ms}/{gpu_compute_submit_budget}, task={task_ms}/{task_budget})"
    )
PY
