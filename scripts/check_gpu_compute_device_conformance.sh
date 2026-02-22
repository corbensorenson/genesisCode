#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

OUT_DIR="${GENESIS_GPU_DEVICE_CONFORMANCE_OUT_DIR:-.genesis/perf/gpu_device_conformance}"
REPORT_OUT="${GENESIS_GPU_DEVICE_CONFORMANCE_REPORT_OUT:-.genesis/perf/gpu_device_conformance_report.json}"
CARGO_PROFILE="${GENESIS_PERF_CARGO_PROFILE:-selfhost-strict}"
MICROBENCH_FEATURES="${GENESIS_GPU_DEVICE_CONFORMANCE_FEATURES:-device-bridge}"
LANE_ID="${GENESIS_GPU_DEVICE_CONFORMANCE_LANE_ID:-}"
GPU_VENDOR="${GENESIS_GPU_DEVICE_CONFORMANCE_VENDOR:-}"
OS_FAMILY="${GENESIS_GPU_DEVICE_CONFORMANCE_OS:-}"

RUNTIME_METRICS_OUT="$OUT_DIR/runtime_microbench_metrics.json"
SLO_OUT="$OUT_DIR/concurrency_gpu_slo_report.json"
PROFILE_OUT="$OUT_DIR/gpu_compute_runtime_profile.json"
PROFILE_GUARD_OUT="$OUT_DIR/gpu_compute_runtime_profile_guard.json"

mkdir -p "$OUT_DIR"

echo "gpu-device-conformance: running runtime microbench (require-device)"
GENESIS_RUNTIME_MICROBENCH_OUT="$RUNTIME_METRICS_OUT" \
  GENESIS_CONCURRENCY_GPU_SLO_OUT="$SLO_OUT" \
  GENESIS_RUNTIME_MICROBENCH_FEATURES="$MICROBENCH_FEATURES" \
  GENESIS_RUNTIME_MICROBENCH_REQUIRED_GPU_BACKEND="device-runtime" \
  GENESIS_GPU_COMPUTE_BACKEND_POLICY="require-device" \
  GENESIS_PERF_CARGO_PROFILE="$CARGO_PROFILE" \
  bash scripts/check_runtime_microbench_budgets.sh

echo "gpu-device-conformance: running compute-only profile guard (require-device)"
GENESIS_GPU_COMPUTE_RUNTIME_PROFILE_OUT="$PROFILE_OUT" \
  GENESIS_GPU_COMPUTE_RUNTIME_PROFILE_GUARD_OUT="$PROFILE_GUARD_OUT" \
  GENESIS_GPU_COMPUTE_RUNTIME_PROFILE_REQUIRED_BACKEND="device-runtime" \
  GENESIS_RUNTIME_MICROBENCH_FEATURES="$MICROBENCH_FEATURES" \
  GENESIS_GPU_COMPUTE_BACKEND_POLICY="require-device" \
  GENESIS_PERF_CARGO_PROFILE="$CARGO_PROFILE" \
  bash scripts/check_gpu_compute_runtime_profile.sh

python3 - "$RUNTIME_METRICS_OUT" "$SLO_OUT" "$PROFILE_OUT" "$PROFILE_GUARD_OUT" "$OUT_DIR" "$REPORT_OUT" "$LANE_ID" "$GPU_VENDOR" "$OS_FAMILY" <<'PY'
import json
import pathlib
import platform
import re
import shutil
import sys

metrics_path = pathlib.Path(sys.argv[1])
slo_path = pathlib.Path(sys.argv[2])
profile_path = pathlib.Path(sys.argv[3])
profile_guard_path = pathlib.Path(sys.argv[4])
out_dir = pathlib.Path(sys.argv[5])
report_out = pathlib.Path(sys.argv[6])
lane_id_override = sys.argv[7].strip().lower()
vendor_override = sys.argv[8].strip().lower()
os_override = sys.argv[9].strip().lower()

metrics = json.loads(metrics_path.read_text(encoding="utf-8"))
slo = json.loads(slo_path.read_text(encoding="utf-8"))
profile = json.loads(profile_path.read_text(encoding="utf-8"))
profile_guard = json.loads(profile_guard_path.read_text(encoding="utf-8"))

backend = str(metrics.get("gpu_compute_backend", "unknown")).strip().lower()
if backend == "device-bridge":
    backend = "device-runtime"
if backend != "device-runtime":
    raise SystemExit(
        f"gpu-device-conformance: expected device-runtime backend, observed {backend!r}"
    )

adapter_raw = metrics.get("gpu_compute_adapter")
if not isinstance(adapter_raw, str) or not adapter_raw.strip():
    raise SystemExit(
        "gpu-device-conformance: runtime microbench report must include non-empty gpu_compute_adapter in require-device mode"
    )
adapter = adapter_raw.strip()
adapter_slug = re.sub(r"[^a-zA-Z0-9._-]+", "-", adapter.lower()).strip("-")
if not adapter_slug:
    adapter_slug = "adapter"

def detect_vendor(adapter_name: str) -> str:
    normalized = adapter_name.lower()
    if "nvidia" in normalized or "geforce" in normalized or "quadro" in normalized:
        return "nvidia"
    if "amd" in normalized or "radeon" in normalized:
        return "amd"
    if "intel" in normalized or "iris" in normalized or "uhd" in normalized:
        return "intel"
    if "apple" in normalized or normalized.startswith("m1") or normalized.startswith("m2"):
        return "apple"
    return "unknown"

def detect_os() -> str:
    p = platform.system().strip().lower()
    if p == "darwin":
        return "macos"
    if p.startswith("win"):
        return "windows"
    if p == "linux":
        return "linux"
    return p or "unknown"

gpu_vendor = vendor_override or detect_vendor(adapter)
os_family = os_override or detect_os()
lane_id = lane_id_override or f"{gpu_vendor}-{os_family}"
lane_slug = re.sub(r"[^a-zA-Z0-9._-]+", "-", lane_id.lower()).strip("-")
if not lane_slug:
    lane_slug = f"{gpu_vendor}-{os_family}-{adapter_slug}"

adapter_metrics_path = out_dir / f"runtime_microbench_metrics.{adapter_slug}.json"
adapter_slo_path = out_dir / f"concurrency_gpu_slo_report.{adapter_slug}.json"
adapter_profile_path = out_dir / f"gpu_compute_runtime_profile.{adapter_slug}.json"
adapter_profile_guard_path = out_dir / f"gpu_compute_runtime_profile_guard.{adapter_slug}.json"
adapter_summary_path = out_dir / f"gpu_device_conformance.{adapter_slug}.json"
lane_summary_path = out_dir / f"gpu_device_conformance.{lane_slug}.json"

for src, dst in [
    (metrics_path, adapter_metrics_path),
    (slo_path, adapter_slo_path),
    (profile_path, adapter_profile_path),
    (profile_guard_path, adapter_profile_guard_path),
]:
    shutil.copy2(src, dst)

summary = {
    "kind": "genesis/gpu-device-conformance-v0.1",
    "lane_id": lane_id,
    "lane_slug": lane_slug,
    "gpu_vendor": gpu_vendor,
    "os_family": os_family,
    "gpu_compute_backend": backend,
    "gpu_compute_adapter": adapter,
    "gpu_compute_adapter_slug": adapter_slug,
    "ok": bool(slo.get("ok", False)) and bool(profile_guard.get("ok", False)),
    "artifacts": {
        "runtime_microbench_metrics": str(adapter_metrics_path),
        "concurrency_gpu_slo_report": str(adapter_slo_path),
        "gpu_compute_runtime_profile": str(adapter_profile_path),
        "gpu_compute_runtime_profile_guard": str(adapter_profile_guard_path),
    },
}

if not summary["ok"]:
    raise SystemExit(
        "gpu-device-conformance: upstream guard reports must both be ok "
        f"(slo_ok={slo.get('ok')}, profile_ok={profile_guard.get('ok')})"
    )

adapter_summary_path.write_text(
    json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8"
)
lane_summary_path.write_text(
    json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8"
)
report_out.parent.mkdir(parents=True, exist_ok=True)
report_out.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")

print(f"gpu-device-conformance: wrote adapter report {adapter_summary_path}")
print(f"gpu-device-conformance: wrote lane report {lane_summary_path}")
print(f"gpu-device-conformance: wrote summary report {report_out}")
PY

echo "gpu-device-conformance: ok"
