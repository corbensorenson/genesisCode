#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

MATRIX_CONFIG="${GENESIS_GPU_DEVICE_MATRIX_CONFIG:-policies/perf/gpu_device_conformance_matrix.toml}"
OUT_PATH="${GENESIS_GPU_DEVICE_MATRIX_REPORT_OUT:-.genesis/perf/gpu_device_conformance_matrix_report.json}"

usage() {
  cat <<'EOF'
Usage: scripts/check_gpu_device_conformance_matrix.sh [--config <matrix.toml>] [--out <report.json>] --lane <lane-id>=<report.json> [--lane <lane-id>=<report.json> ...]
EOF
}

LANE_ARGS=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --config)
      MATRIX_CONFIG="${2:-}"
      shift 2
      ;;
    --out)
      OUT_PATH="${2:-}"
      shift 2
      ;;
    --lane)
      LANE_ARGS+=("${2:-}")
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "gpu-device-matrix: unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [[ "${#LANE_ARGS[@]}" -eq 0 ]]; then
  echo "gpu-device-matrix: at least one --lane <id>=<report.json> is required" >&2
  usage >&2
  exit 2
fi

python3 - "$MATRIX_CONFIG" "$OUT_PATH" "${LANE_ARGS[@]}" <<'PY'
import json
import pathlib
import re
import sys

config_path = pathlib.Path(sys.argv[1])
out_path = pathlib.Path(sys.argv[2])
lane_specs = sys.argv[3:]

if not config_path.is_file():
    raise SystemExit(f"gpu-device-matrix: missing config: {config_path}")

def parse_matrix_lanes(path):
    lanes = []
    cur = None
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.split("#", 1)[0].strip()
        if not line:
            continue
        if line == "[[lane]]":
            if cur is not None:
                lanes.append(cur)
            cur = {}
            continue
        if "=" not in line:
            continue
        if cur is None:
            continue
        key, value = line.split("=", 1)
        key = key.strip()
        value = value.strip()
        if value.startswith('"') and value.endswith('"'):
            value = value[1:-1]
        cur[key] = value
    if cur is not None:
        lanes.append(cur)
    return lanes

lanes_cfg = parse_matrix_lanes(config_path)
if not lanes_cfg:
    raise SystemExit("gpu-device-matrix: config must define at least one [[lane]] entry")

required_lanes = {}
for idx, lane in enumerate(lanes_cfg):
    if not isinstance(lane, dict):
        raise SystemExit(f"gpu-device-matrix: lane[{idx}] must be a table")
    lane_id = str(lane.get("id", "")).strip().lower()
    vendor = str(lane.get("vendor", "")).strip().lower()
    os_family = str(lane.get("os", "")).strip().lower()
    if not lane_id or not vendor or not os_family:
        raise SystemExit(
            f"gpu-device-matrix: lane[{idx}] requires non-empty id/vendor/os"
        )
    if lane_id in required_lanes:
        raise SystemExit(f"gpu-device-matrix: duplicate lane id in config: {lane_id}")
    required_lanes[lane_id] = {"vendor": vendor, "os": os_family}

provided = {}
for raw in lane_specs:
    if "=" not in raw:
        raise SystemExit(
            f"gpu-device-matrix: invalid --lane spec {raw!r}; expected <lane-id>=<report.json>"
        )
    lane_id, path = raw.split("=", 1)
    lane_id = lane_id.strip().lower()
    path = path.strip()
    if not lane_id or not path:
        raise SystemExit(
            f"gpu-device-matrix: invalid --lane spec {raw!r}; expected <lane-id>=<report.json>"
        )
    if lane_id in provided:
        raise SystemExit(f"gpu-device-matrix: duplicate provided lane id: {lane_id}")
    provided[lane_id] = pathlib.Path(path)

missing = sorted(set(required_lanes) - set(provided))
if missing:
    raise SystemExit(f"gpu-device-matrix: missing required lane reports: {missing}")

EXPECTED_KIND = "genesis/gpu-device-conformance-v0.1"
EXPECTED_BACKEND = "device-runtime"
REQUIRED_ARTIFACT_KEYS = {
    "runtime_microbench_metrics",
    "concurrency_gpu_slo_report",
    "gpu_compute_runtime_profile",
    "gpu_compute_runtime_profile_guard",
}

lane_summaries = {}
contract_ref = None
for lane_id in sorted(required_lanes):
    report_path = provided[lane_id]
    if not report_path.is_file():
        raise SystemExit(f"gpu-device-matrix: missing lane report for {lane_id}: {report_path}")
    doc = json.loads(report_path.read_text(encoding="utf-8"))
    if not isinstance(doc, dict):
        raise SystemExit(f"gpu-device-matrix: lane {lane_id} report must be JSON object")
    if doc.get("kind") != EXPECTED_KIND:
        raise SystemExit(
            f"gpu-device-matrix: lane {lane_id} bad kind {doc.get('kind')!r}"
        )
    if doc.get("ok") is not True:
        raise SystemExit(f"gpu-device-matrix: lane {lane_id} not ok")
    if doc.get("gpu_compute_backend") != EXPECTED_BACKEND:
        raise SystemExit(
            f"gpu-device-matrix: lane {lane_id} backend must be {EXPECTED_BACKEND!r}"
        )
    if str(doc.get("lane_id", "")).strip().lower() != lane_id:
        raise SystemExit(
            f"gpu-device-matrix: lane {lane_id} report lane_id mismatch ({doc.get('lane_id')!r})"
        )
    expected = required_lanes[lane_id]
    if str(doc.get("gpu_vendor", "")).strip().lower() != expected["vendor"]:
        raise SystemExit(
            f"gpu-device-matrix: lane {lane_id} vendor mismatch (expected={expected['vendor']!r}, got={doc.get('gpu_vendor')!r})"
        )
    if str(doc.get("os_family", "")).strip().lower() != expected["os"]:
        raise SystemExit(
            f"gpu-device-matrix: lane {lane_id} os mismatch (expected={expected['os']!r}, got={doc.get('os_family')!r})"
        )
    adapter = doc.get("gpu_compute_adapter")
    adapter_slug = str(doc.get("gpu_compute_adapter_slug", "")).strip().lower()
    if not isinstance(adapter, str) or not adapter.strip():
        raise SystemExit(
            f"gpu-device-matrix: lane {lane_id} missing non-empty gpu_compute_adapter"
        )
    if not re.fullmatch(r"[a-z0-9._-]+", adapter_slug):
        raise SystemExit(
            f"gpu-device-matrix: lane {lane_id} invalid adapter slug {adapter_slug!r}"
        )
    artifacts = doc.get("artifacts")
    if not isinstance(artifacts, dict):
        raise SystemExit(f"gpu-device-matrix: lane {lane_id} artifacts must be an object")
    keys = set(artifacts.keys())
    if keys != REQUIRED_ARTIFACT_KEYS:
        raise SystemExit(
            f"gpu-device-matrix: lane {lane_id} artifact key mismatch (expected={sorted(REQUIRED_ARTIFACT_KEYS)} got={sorted(keys)})"
        )
    for key in sorted(REQUIRED_ARTIFACT_KEYS):
        v = artifacts.get(key)
        if not isinstance(v, str) or not v.strip():
            raise SystemExit(
                f"gpu-device-matrix: lane {lane_id} artifact path for {key!r} must be non-empty string"
            )
        if f".{adapter_slug}.json" not in v:
            raise SystemExit(
                f"gpu-device-matrix: lane {lane_id} artifact path for {key!r} must retain adapter-specific suffix .{adapter_slug}.json (got={v!r})"
            )

    contract = {
        "kind": doc["kind"],
        "backend": doc["gpu_compute_backend"],
        "artifact_keys": sorted(keys),
    }
    if contract_ref is None:
        contract_ref = contract
    elif contract != contract_ref:
        raise SystemExit(
            f"gpu-device-matrix: lane {lane_id} contract mismatch (expected={contract_ref}, got={contract})"
        )

    lane_summaries[lane_id] = {
        "report": str(report_path),
        "gpu_vendor": expected["vendor"],
        "os_family": expected["os"],
        "gpu_compute_adapter": adapter,
        "gpu_compute_adapter_slug": adapter_slug,
    }

summary = {
    "kind": "genesis/gpu-device-conformance-matrix-v0.1",
    "ok": True,
    "config": str(config_path),
    "contract": contract_ref,
    "lanes": lane_summaries,
}

out_path.parent.mkdir(parents=True, exist_ok=True)
out_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"gpu-device-matrix: wrote report {out_path}")
print(f"gpu-device-matrix: ok lanes={sorted(lane_summaries.keys())}")
PY
