#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

OUT="${GENESIS_GPU_COMPUTE_RUNTIME_PROFILE_OUT:-.genesis/perf/gpu_compute_runtime_profile.json}"
SUMMARY_OUT="${GENESIS_GPU_COMPUTE_RUNTIME_PROFILE_GUARD_OUT:-.genesis/perf/gpu_compute_runtime_profile_guard.json}"
SKIP_RUN="${GENESIS_GPU_COMPUTE_RUNTIME_PROFILE_SKIP_RUN:-0}"
CARGO_PROFILE="${GENESIS_PERF_CARGO_PROFILE:-selfhost-strict}"
MICROBENCH_FEATURES="${GENESIS_RUNTIME_MICROBENCH_FEATURES:-}"
REQUIRED_BACKEND="${GENESIS_GPU_COMPUTE_RUNTIME_PROFILE_REQUIRED_BACKEND:-}"
GPU_BACKEND_POLICY="${GENESIS_GPU_COMPUTE_BACKEND_POLICY:-dev-allow-fallback}"

if [[ "$SKIP_RUN" == "1" ]]; then
  if [[ ! -f "$OUT" ]]; then
    echo "gpu-compute-runtime-profile: GENESIS_GPU_COMPUTE_RUNTIME_PROFILE_SKIP_RUN=1 requires existing report: $OUT" >&2
    exit 2
  fi
  echo "gpu-compute-runtime-profile: skipping benchmark execution (GENESIS_GPU_COMPUTE_RUNTIME_PROFILE_SKIP_RUN=1)"
else
  echo "gpu-compute-runtime-profile: running compute-only runtime microbench profile"
  if [[ -n "$MICROBENCH_FEATURES" ]]; then
    GENESIS_RUNTIME_MICROBENCH_PROFILE="$CARGO_PROFILE" \
      GENESIS_RUNTIME_MICROBENCH_BUILD_MODE="release-equivalent" \
      GENESIS_GPU_COMPUTE_BACKEND_POLICY="$GPU_BACKEND_POLICY" \
      cargo run --profile "$CARGO_PROFILE" -p gc_runtime_bench --features "$MICROBENCH_FEATURES" -- --mode compute-only --out "$OUT"
  else
    GENESIS_RUNTIME_MICROBENCH_PROFILE="$CARGO_PROFILE" \
      GENESIS_RUNTIME_MICROBENCH_BUILD_MODE="release-equivalent" \
      GENESIS_GPU_COMPUTE_BACKEND_POLICY="$GPU_BACKEND_POLICY" \
      cargo run --profile "$CARGO_PROFILE" -p gc_runtime_bench -- --mode compute-only --out "$OUT"
  fi
fi

python3 - "$OUT" "$SUMMARY_OUT" "$REQUIRED_BACKEND" <<'PY'
import json
import pathlib
import sys

metrics_path = pathlib.Path(sys.argv[1])
summary_path = pathlib.Path(sys.argv[2])
required_backend = sys.argv[3].strip()

def normalize_backend(raw: str) -> str:
    backend = raw.strip().lower()
    if backend == "device-bridge":
        return "device-runtime"
    return backend

doc = json.loads(metrics_path.read_text(encoding="utf-8"))
kind = str(doc.get("kind", ""))
bench_mode = str(doc.get("bench_mode", ""))
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
metrics = doc.get("metrics")
budgets = doc.get("budgets")

if kind != "genesis/runtime-microbench-v0.1":
    raise SystemExit(
        f"gpu-compute-runtime-profile: unexpected report kind {kind!r} "
        "(expected genesis/runtime-microbench-v0.1)"
    )
if bench_mode != "compute-only":
    raise SystemExit(
        f"gpu-compute-runtime-profile: bench_mode must be compute-only, got {bench_mode!r}"
    )
if not isinstance(metrics, dict) or not isinstance(budgets, dict):
    raise SystemExit("gpu-compute-runtime-profile: missing metrics/budgets map")
if gpu_compute_backend_policy not in {"dev-allow-fallback", "require-device"}:
    raise SystemExit(
        f"gpu-compute-runtime-profile: unexpected gpu_compute_backend_policy {gpu_compute_backend_policy!r}"
    )

gpu_metric = int(metrics.get("gpu_compute_submit_ms", -1))
gpu_budget = int(budgets.get("gpu_compute_submit_ms", -1))
if gpu_metric < 0 or gpu_budget <= 0:
    raise SystemExit("gpu-compute-runtime-profile: invalid gpu_compute_submit_ms metric/budget")

non_compute_keys = [
    "eval_ms",
    "runner_ms",
    "bridge_runner_ms",
    "task_runner_ms",
    "patch_apply_ms",
    "store_cycle_ms",
    "sync_pull_ms",
]
non_compute_nonzero = {}
for key in non_compute_keys:
    value = int(metrics.get(key, -1))
    if value < 0:
        raise SystemExit(f"gpu-compute-runtime-profile: missing metrics.{key}")
    if value != 0:
        non_compute_nonzero[key] = value

backend_ok = (not required_backend_normalized) or gpu_compute_backend == required_backend_normalized
gpu_budget_ok = gpu_metric <= gpu_budget
non_compute_ok = not non_compute_nonzero
ok = backend_ok and gpu_budget_ok and non_compute_ok

summary = {
    "kind": "genesis/gpu-compute-runtime-profile-guard-v0.1",
    "source_report": str(metrics_path),
    "bench_mode": bench_mode,
    "gpu_compute_backend": gpu_compute_backend,
    "gpu_compute_backend_raw": gpu_compute_backend_raw,
    "gpu_compute_backend_policy": gpu_compute_backend_policy,
    "gpu_compute_adapter": gpu_compute_adapter,
    "required_backend": required_backend_normalized or None,
    "required_backend_raw": required_backend or None,
    "gpu_compute_submit_ms": gpu_metric,
    "gpu_compute_submit_budget_ms": gpu_budget,
    "gpu_compute_submit_ok": gpu_budget_ok,
    "non_compute_metrics_zero": non_compute_ok,
    "non_compute_nonzero": non_compute_nonzero,
    "backend_ok": backend_ok,
    "ok": ok,
}

summary_path.parent.mkdir(parents=True, exist_ok=True)
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"gpu-compute-runtime-profile: wrote guard report {summary_path}")

if not ok:
    raise SystemExit(
        "gpu-compute-runtime-profile: guard failure "
        f"(backend={gpu_compute_backend}, required={required_backend_normalized or 'any'}, "
        f"gpu_compute_submit={gpu_metric}/{gpu_budget}, non_compute_nonzero={non_compute_nonzero})"
    )
PY

echo "gpu-compute-runtime-profile: ok"
