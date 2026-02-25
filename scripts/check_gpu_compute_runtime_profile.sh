#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
source "$ROOT_DIR/scripts/lib/profile_gate_timing.sh"
source "$ROOT_DIR/scripts/lib/perf_disk_mode.sh"
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "check-gpu-compute-runtime-profile" \
  ".genesis/build/cargo" \
  "GENESIS_CHECK_GPU_COMPUTE_RUNTIME_PROFILE_CARGO_TARGET_DIR"

START_MS="$(genesis_profile_gate_now_ms)"

OUT="${GENESIS_GPU_COMPUTE_RUNTIME_PROFILE_OUT:-.genesis/perf/gpu_compute_runtime_profile.json}"
SUMMARY_OUT="${GENESIS_GPU_COMPUTE_RUNTIME_PROFILE_GUARD_OUT:-.genesis/perf/gpu_compute_runtime_profile_guard.json}"
SKIP_RUN="${GENESIS_GPU_COMPUTE_RUNTIME_PROFILE_SKIP_RUN:-0}"
CARGO_PROFILE="${GENESIS_PERF_CARGO_PROFILE:-selfhost-strict}"
MICROBENCH_FEATURES="${GENESIS_RUNTIME_MICROBENCH_FEATURES:-}"
REQUIRED_BACKEND="${GENESIS_GPU_COMPUTE_RUNTIME_PROFILE_REQUIRED_BACKEND:-}"
GPU_BACKEND_POLICY="${GENESIS_GPU_COMPUTE_BACKEND_POLICY:-}"
DISK_STRICT_MODE="$(genesis_resolve_perf_disk_strict_mode)"
DISK_MIN_FREE_KB="${GENESIS_GPU_COMPUTE_RUNTIME_PROFILE_MIN_FREE_KB:-3145728}"
RUNTIME_REPORT="${GENESIS_GPU_COMPUTE_RUNTIME_PROFILE_RUNTIME_REPORT_OUT:-.genesis/perf/gpu_compute_runtime_profile_runtime_report.json}"
RUNTIME_HISTORY="${GENESIS_GPU_COMPUTE_RUNTIME_PROFILE_RUNTIME_HISTORY_OUT:-.genesis/perf/gpu_compute_runtime_profile_runtime_history.jsonl}"
RUNTIME_BASELINE_HISTORY="${GENESIS_GPU_COMPUTE_RUNTIME_PROFILE_RUNTIME_BASELINE_HISTORY_OUT:-policies/perf/gpu_compute_runtime_profile_runtime_seed_history.jsonl}"
RUNTIME_BUDGET_MS="${GENESIS_GPU_COMPUTE_RUNTIME_PROFILE_RUNTIME_BUDGET_MS:-900000}"
RUNTIME_MIN_HISTORY="${GENESIS_GPU_COMPUTE_RUNTIME_PROFILE_RUNTIME_MIN_HISTORY:-5}"
RUNTIME_REQUIRE_MIN_HISTORY="${GENESIS_GPU_COMPUTE_RUNTIME_PROFILE_RUNTIME_REQUIRE_MIN_HISTORY:-1}"
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
if [[ -z "$REQUIRED_BACKEND" && "$GPU_BACKEND_POLICY" == "require-device" ]]; then
  REQUIRED_BACKEND="device-runtime"
fi
if [[ -z "$MICROBENCH_FEATURES" && "$GPU_BACKEND_POLICY" == "require-device" ]]; then
  MICROBENCH_FEATURES="device-bridge"
fi
if [[ ! "$RUNTIME_MIN_HISTORY" =~ ^[0-9]+$ || "$RUNTIME_MIN_HISTORY" -le 0 ]]; then
  echo "gpu-compute-runtime-profile: GENESIS_GPU_COMPUTE_RUNTIME_PROFILE_RUNTIME_MIN_HISTORY must be a positive integer" >&2
  exit 2
fi
if [[ "$RUNTIME_REQUIRE_MIN_HISTORY" != "0" && "$RUNTIME_REQUIRE_MIN_HISTORY" != "1" ]]; then
  echo "gpu-compute-runtime-profile: GENESIS_GPU_COMPUTE_RUNTIME_PROFILE_RUNTIME_REQUIRE_MIN_HISTORY must be 0 or 1" >&2
  exit 2
fi

bash scripts/check_disk_headroom.sh \
  --path "$ROOT_DIR" \
  --context "gpu-compute-runtime-profile" \
  --min-kb "$DISK_MIN_FREE_KB" \
  --strict "$DISK_STRICT_MODE"

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

genesis_profile_gate_emit_runtime_report \
  "gpu-compute-runtime-profile" \
  "genesis/gpu-compute-runtime-profile-runtime-v0.1" \
  "$RUNTIME_REPORT" \
  "$RUNTIME_HISTORY" \
  "$START_MS" \
  "$RUNTIME_BUDGET_MS" \
  "$RUNTIME_MIN_HISTORY" \
  "{\"metrics_report\":\"$OUT\",\"guard_report\":\"$SUMMARY_OUT\",\"build_profile\":\"$CARGO_PROFILE\"}" \
  "" \
  "$RUNTIME_BASELINE_HISTORY" \
  "$RUNTIME_REQUIRE_MIN_HISTORY"

echo "gpu-compute-runtime-profile: ok"
