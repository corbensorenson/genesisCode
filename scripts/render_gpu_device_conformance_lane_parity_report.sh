#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if [[ "$#" -ne 3 ]]; then
  echo "usage: $0 <lane-a-report> <lane-b-report> <parity-report-output>" >&2
  exit 2
fi

LANE_A="$1"
LANE_B="$2"
OUT_PATH="$3"

python3 - "$LANE_A" "$LANE_B" "$OUT_PATH" <<'PY'
import json
import pathlib
import sys

lane_a_path = pathlib.Path(sys.argv[1])
lane_b_path = pathlib.Path(sys.argv[2])
out_path = pathlib.Path(sys.argv[3])

for path in (lane_a_path, lane_b_path):
    if not path.is_file():
        raise SystemExit(f"gpu-device-lane-parity: missing conformance report: {path}")

lane_a = json.loads(lane_a_path.read_text(encoding="utf-8"))
lane_b = json.loads(lane_b_path.read_text(encoding="utf-8"))

EXPECTED_KIND = "genesis/gpu-device-conformance-v0.1"
EXPECTED_BACKEND = "device-runtime"
REQUIRED_ARTIFACT_KEYS = {
    "runtime_microbench_metrics",
    "concurrency_gpu_slo_report",
    "gpu_compute_runtime_profile",
    "gpu_compute_runtime_profile_guard",
}

def validate_lane(name: str, doc: dict) -> dict:
    if not isinstance(doc, dict):
        raise SystemExit(f"gpu-device-lane-parity: lane {name} report must be a JSON object")
    if doc.get("kind") != EXPECTED_KIND:
        raise SystemExit(
            f"gpu-device-lane-parity: lane {name} has unexpected kind {doc.get('kind')!r}"
        )
    if doc.get("ok") is not True:
        raise SystemExit(f"gpu-device-lane-parity: lane {name} is not ok")
    if doc.get("gpu_compute_backend") != EXPECTED_BACKEND:
        raise SystemExit(
            f"gpu-device-lane-parity: lane {name} backend must be {EXPECTED_BACKEND!r}, got {doc.get('gpu_compute_backend')!r}"
        )
    adapter = doc.get("gpu_compute_adapter")
    if not isinstance(adapter, str) or not adapter.strip():
        raise SystemExit(
            f"gpu-device-lane-parity: lane {name} must include non-empty gpu_compute_adapter"
        )
    artifacts = doc.get("artifacts")
    if not isinstance(artifacts, dict):
        raise SystemExit(
            f"gpu-device-lane-parity: lane {name} artifacts must be a JSON object"
        )
    keys = set(artifacts.keys())
    if keys != REQUIRED_ARTIFACT_KEYS:
        raise SystemExit(
            f"gpu-device-lane-parity: lane {name} artifact key mismatch (expected={sorted(REQUIRED_ARTIFACT_KEYS)} got={sorted(keys)})"
        )
    for k in sorted(REQUIRED_ARTIFACT_KEYS):
        v = artifacts[k]
        if not isinstance(v, str) or not v.strip():
            raise SystemExit(
                f"gpu-device-lane-parity: lane {name} artifact path for {k!r} must be non-empty string"
            )
    return {
        "kind": doc["kind"],
        "gpu_compute_backend": doc["gpu_compute_backend"],
        "artifact_keys": sorted(keys),
    }

a_contract = validate_lane("a", lane_a)
b_contract = validate_lane("b", lane_b)

if a_contract != b_contract:
    raise SystemExit(
        "gpu-device-lane-parity: contract mismatch between lanes "
        f"(lane_a={a_contract}, lane_b={b_contract})"
    )

summary = {
    "kind": "genesis/gpu-device-conformance-lane-parity-v0.1",
    "ok": True,
    "lane_a_report": str(lane_a_path),
    "lane_b_report": str(lane_b_path),
    "contract": a_contract,
    "lane_a_adapter": lane_a.get("gpu_compute_adapter"),
    "lane_b_adapter": lane_b.get("gpu_compute_adapter"),
}

out_path.parent.mkdir(parents=True, exist_ok=True)
out_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"gpu-device-lane-parity: wrote report {out_path}")
print(
    "gpu-device-lane-parity: ok "
    f"backend={a_contract['gpu_compute_backend']} "
    f"lane_a_adapter={summary['lane_a_adapter']} "
    f"lane_b_adapter={summary['lane_b_adapter']}"
)
PY
