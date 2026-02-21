#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

OUT="${GENESIS_RUNTIME_MICROBENCH_OUT:-.genesis/perf/runtime_microbench_metrics.json}"
SLO_OUT="${GENESIS_CONCURRENCY_GPU_SLO_OUT:-.genesis/perf/concurrency_gpu_slo_report.json}"
SKIP_RUN="${GENESIS_RUNTIME_MICROBENCH_SKIP_RUN:-0}"
REQUIRED_GPU_BACKEND="${GENESIS_RUNTIME_MICROBENCH_REQUIRED_GPU_BACKEND:-}"
GPU_BUDGET_DEVICE_MS="${GENESIS_BUDGET_MICRO_GPU_COMPUTE_SUBMIT_MS_DEVICE:-5000}"
GPU_BUDGET_FALLBACK_MS="${GENESIS_BUDGET_MICRO_GPU_COMPUTE_SUBMIT_MS_FALLBACK:-8000}"
CARGO_PROFILE="${GENESIS_PERF_CARGO_PROFILE:-selfhost-strict}"
DISK_STRICT_MODE="${GENESIS_PERF_DISK_STRICT_MODE:-1}"
MICROBENCH_FEATURES="${GENESIS_RUNTIME_MICROBENCH_FEATURES:-}"
GPU_BACKEND_POLICY="${GENESIS_GPU_COMPUTE_BACKEND_POLICY:-}"
if [[ -z "$GPU_BACKEND_POLICY" ]]; then
  case "$CARGO_PROFILE" in
    selfhost-strict|release|release-*|production|prod)
      GPU_BACKEND_POLICY="require-device"
      ;;
    *)
      GPU_BACKEND_POLICY="dev-allow-fallback"
      ;;
  esac
fi
if [[ -z "$REQUIRED_GPU_BACKEND" && "$GPU_BACKEND_POLICY" == "require-device" ]]; then
  REQUIRED_GPU_BACKEND="device-runtime"
fi
if [[ -z "$MICROBENCH_FEATURES" && "$GPU_BACKEND_POLICY" == "require-device" ]]; then
  MICROBENCH_FEATURES="device-bridge"
fi

bash scripts/check_disk_headroom.sh --path "$ROOT_DIR" --context "runtime-microbench" --strict "$DISK_STRICT_MODE"

if [[ "$SKIP_RUN" == "1" ]]; then
  if [[ ! -f "$OUT" ]]; then
    echo "runtime-microbench: GENESIS_RUNTIME_MICROBENCH_SKIP_RUN=1 requires existing report: $OUT" >&2
    exit 2
  fi
  echo "runtime-microbench: skipping benchmark execution (GENESIS_RUNTIME_MICROBENCH_SKIP_RUN=1)"
else
  echo "runtime-microbench: running benchmark suite"
  if [[ -n "$MICROBENCH_FEATURES" ]]; then
    GENESIS_RUNTIME_MICROBENCH_PROFILE="$CARGO_PROFILE" \
      GENESIS_RUNTIME_MICROBENCH_BUILD_MODE="release-equivalent" \
      GENESIS_GPU_COMPUTE_BACKEND_POLICY="$GPU_BACKEND_POLICY" \
      cargo run --profile "$CARGO_PROFILE" -p gc_runtime_bench --features "$MICROBENCH_FEATURES" -- --out "$OUT"
  else
    GENESIS_RUNTIME_MICROBENCH_PROFILE="$CARGO_PROFILE" \
      GENESIS_RUNTIME_MICROBENCH_BUILD_MODE="release-equivalent" \
      GENESIS_GPU_COMPUTE_BACKEND_POLICY="$GPU_BACKEND_POLICY" \
      cargo run --profile "$CARGO_PROFILE" -p gc_runtime_bench -- --out "$OUT"
  fi
fi

echo "runtime-microbench: metrics"
cat "$OUT"

python3 - "$OUT" "$SLO_OUT" "$REQUIRED_GPU_BACKEND" "$GPU_BUDGET_DEVICE_MS" "$GPU_BUDGET_FALLBACK_MS" "$CARGO_PROFILE" "$DISK_STRICT_MODE" <<'PY'
import json
import pathlib
import sys

metrics_path = pathlib.Path(sys.argv[1])
slo_path = pathlib.Path(sys.argv[2])
required_backend = sys.argv[3].strip()
device_budget = int(sys.argv[4])
fallback_budget = int(sys.argv[5])
build_profile = sys.argv[6]
disk_strict_mode = sys.argv[7]

doc = json.loads(metrics_path.read_text(encoding="utf-8"))
metrics = doc.get("metrics")
budgets = doc.get("budgets")

def normalize_backend(raw: str) -> str:
    backend = raw.strip().lower()
    if backend == "device-bridge":
        return "device-runtime"
    return backend

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
task_ms = int(metrics["task_runner_ms"])
task_budget = int(budgets["task_runner_ms"])
gpu_compute_backend_raw = str(doc.get("gpu_compute_backend", "unknown"))
gpu_compute_backend = normalize_backend(gpu_compute_backend_raw)
gpu_compute_backend_policy = str(doc.get("gpu_compute_backend_policy", "unknown"))
gpu_compute_adapter_raw = doc.get("gpu_compute_adapter")
gpu_compute_adapter = None
if isinstance(gpu_compute_adapter_raw, str):
    trimmed = gpu_compute_adapter_raw.strip()
    if trimmed:
        gpu_compute_adapter = trimmed
required_backend_normalized = normalize_backend(required_backend) if required_backend else ""
if gpu_compute_backend_policy not in {"dev-allow-fallback", "require-device"}:
    raise SystemExit(
        f"runtime-microbench: unexpected gpu_compute_backend_policy {gpu_compute_backend_policy!r}"
    )

if gpu_compute_backend == "device-runtime":
    gpu_compute_submit_budget = device_budget
else:
    gpu_compute_submit_budget = fallback_budget

bridge_ok = bridge_ms <= bridge_budget
gpu_compute_submit_ok = gpu_compute_submit_ms <= gpu_compute_submit_budget
task_ok = task_ms <= task_budget
backend_ok = (not required_backend_normalized) or gpu_compute_backend == required_backend_normalized
ok = bridge_ok and gpu_compute_submit_ok and task_ok and backend_ok

slo = {
    "kind": "genesis/concurrency-gpu-slo-v0.1",
    "source_report": str(metrics_path),
    "build_profile": str(doc.get("build_profile", build_profile)),
    "build_mode": str(doc.get("build_mode", "release-equivalent")),
    "disk_strict_mode": disk_strict_mode,
    "gpu_compute_backend": gpu_compute_backend,
    "gpu_compute_backend_raw": gpu_compute_backend_raw,
    "gpu_compute_backend_policy": gpu_compute_backend_policy,
    "gpu_compute_adapter": gpu_compute_adapter,
    "gpu_compute_required_backend": required_backend_normalized or None,
    "gpu_compute_required_backend_raw": required_backend or None,
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
            "budget_by_backend_ms": {
                "device-runtime": device_budget,
                "deterministic-fallback": fallback_budget,
            },
            "ok": gpu_compute_submit_ok,
        },
        "gpu_compute_backend_required": {
            "required_backend": required_backend_normalized or None,
            "observed_backend": gpu_compute_backend,
            "ok": backend_ok,
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
    backend_msg = ""
    if required_backend_normalized and gpu_compute_backend != required_backend_normalized:
        backend_msg = (
            f", backend={gpu_compute_backend} (required={required_backend_normalized})"
        )
    raise SystemExit(
        "runtime-microbench: concurrency/gpu slo failure "
        f"(bridge={bridge_ms}/{bridge_budget}, gpu_compute_submit={gpu_compute_submit_ms}/{gpu_compute_submit_budget}, task={task_ms}/{task_budget}{backend_msg})"
    )
PY
