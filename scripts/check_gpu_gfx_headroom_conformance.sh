#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "gpu-gfx-headroom-conformance" \
  ".genesis/build/cargo" \
  "GENESIS_GPU_GFX_HEADROOM_CARGO_TARGET_DIR"

source "$ROOT_DIR/scripts/lib/heavy_gate_preflight.sh"

REPORT_OUT="${GENESIS_GPU_GFX_HEADROOM_REPORT_OUT:-.genesis/perf/gpu_gfx_headroom_conformance_report.json}"
HISTORY_OUT="${GENESIS_GPU_GFX_HEADROOM_HISTORY_OUT:-.genesis/perf/gpu_gfx_headroom_conformance_history.jsonl}"
BUDGET_MS="${GENESIS_GPU_GFX_HEADROOM_BUDGET_MS:-600000}"
TMP_ROOT_BASE="${GENESIS_GPU_GFX_HEADROOM_TMPDIR:-$ROOT_DIR/.genesis/tmp/check-gpu-gfx-headroom-conformance}"
NORMAL_MIN_FREE_KB="${GENESIS_GPU_GFX_HEADROOM_NORMAL_MIN_FREE_KB:-3145728}"
NORMAL_AUTO_RECLAIM="${GENESIS_GPU_GFX_HEADROOM_NORMAL_AUTO_RECLAIM:-1}"
NORMAL_STRICT_MODE="${GENESIS_GPU_GFX_HEADROOM_NORMAL_STRICT_MODE:-auto}"
LOW_AUTO_RECLAIM="${GENESIS_GPU_GFX_HEADROOM_LOW_AUTO_RECLAIM:-0}"
LOW_STRICT_MODE="${GENESIS_GPU_GFX_HEADROOM_LOW_STRICT_MODE:-0}"
LOW_MIN_FREE_KB="${GENESIS_GPU_GFX_HEADROOM_LOW_MIN_FREE_KB:-}"
REQUIRE_DEVICE_LANE="${GENESIS_GPU_GFX_HEADROOM_REQUIRE_DEVICE_LANE:-auto}"
DEVICE_CONFORMANCE_REPORT="${GENESIS_GPU_GFX_HEADROOM_DEVICE_CONFORMANCE_REPORT:-.genesis/perf/gpu_device_conformance_report.json}"
DEVICE_CONFORMANCE_REFRESH="${GENESIS_GPU_GFX_HEADROOM_DEVICE_CONFORMANCE_REFRESH:-1}"
DEVICE_CONFORMANCE_CMD="${GENESIS_GPU_GFX_HEADROOM_DEVICE_CONFORMANCE_CMD:-$ROOT_DIR/scripts/check_gpu_compute_device_conformance.sh}"
GENESIS_BIN="${GENESIS_BIN:-$CARGO_TARGET_DIR/debug/genesis}"

if [[ ! "$BUDGET_MS" =~ ^[0-9]+$ || "$BUDGET_MS" -le 0 ]]; then
  echo "gpu-gfx-headroom: GENESIS_GPU_GFX_HEADROOM_BUDGET_MS must be a positive integer" >&2
  exit 2
fi
if [[ ! "$NORMAL_MIN_FREE_KB" =~ ^[0-9]+$ || "$NORMAL_MIN_FREE_KB" -le 0 ]]; then
  echo "gpu-gfx-headroom: GENESIS_GPU_GFX_HEADROOM_NORMAL_MIN_FREE_KB must be a positive integer" >&2
  exit 2
fi
if [[ "$NORMAL_AUTO_RECLAIM" != "0" && "$NORMAL_AUTO_RECLAIM" != "1" ]]; then
  echo "gpu-gfx-headroom: GENESIS_GPU_GFX_HEADROOM_NORMAL_AUTO_RECLAIM must be 0 or 1" >&2
  exit 2
fi
if [[ "$NORMAL_STRICT_MODE" != "auto" && "$NORMAL_STRICT_MODE" != "0" && "$NORMAL_STRICT_MODE" != "1" ]]; then
  echo "gpu-gfx-headroom: GENESIS_GPU_GFX_HEADROOM_NORMAL_STRICT_MODE must be auto, 0, or 1" >&2
  exit 2
fi
if [[ "$LOW_AUTO_RECLAIM" != "0" && "$LOW_AUTO_RECLAIM" != "1" ]]; then
  echo "gpu-gfx-headroom: GENESIS_GPU_GFX_HEADROOM_LOW_AUTO_RECLAIM must be 0 or 1" >&2
  exit 2
fi
if [[ "$LOW_STRICT_MODE" != "0" && "$LOW_STRICT_MODE" != "1" ]]; then
  echo "gpu-gfx-headroom: GENESIS_GPU_GFX_HEADROOM_LOW_STRICT_MODE must be 0 or 1" >&2
  exit 2
fi
if [[ "$REQUIRE_DEVICE_LANE" != "auto" && "$REQUIRE_DEVICE_LANE" != "0" && "$REQUIRE_DEVICE_LANE" != "1" ]]; then
  echo "gpu-gfx-headroom: GENESIS_GPU_GFX_HEADROOM_REQUIRE_DEVICE_LANE must be auto, 0, or 1" >&2
  exit 2
fi
if [[ "$DEVICE_CONFORMANCE_REFRESH" != "0" && "$DEVICE_CONFORMANCE_REFRESH" != "1" ]]; then
  echo "gpu-gfx-headroom: GENESIS_GPU_GFX_HEADROOM_DEVICE_CONFORMANCE_REFRESH must be 0 or 1" >&2
  exit 2
fi

if [[ ! -x "$GENESIS_BIN" ]]; then
  cargo build -p gc_cli >/dev/null
fi

read_free_kb() {
  df -Pk "$ROOT_DIR" | awk 'NR==2 {print $4}'
}

FREE_KB_START="$(read_free_kb)"
if [[ -z "$LOW_MIN_FREE_KB" ]]; then
  LOW_MIN_FREE_KB=$((FREE_KB_START + 1048576))
fi
if [[ ! "$LOW_MIN_FREE_KB" =~ ^[0-9]+$ || "$LOW_MIN_FREE_KB" -le 0 ]]; then
  echo "gpu-gfx-headroom: GENESIS_GPU_GFX_HEADROOM_LOW_MIN_FREE_KB must be a positive integer" >&2
  exit 2
fi
LOW_HEADROOM_SIMULATED=0
if (( FREE_KB_START < LOW_MIN_FREE_KB )); then
  LOW_HEADROOM_SIMULATED=1
fi

START_MS="$(python3 - <<'PY'
import time
print(int(time.time() * 1000))
PY
)"

TMP_DIR="$(mktemp -d)"
RESULTS_DIR="$TMP_DIR/results"
mkdir -p "$RESULTS_DIR"
trap 'rm -rf "$TMP_DIR"' EXIT

FAILURES=()

resolve_device_runtime_availability() {
  local report_path="$1"
  python3 - "$report_path" <<'PY'
import json
import pathlib
import sys

report_path = pathlib.Path(sys.argv[1])
if not report_path.is_file():
    print("available=0")
    print("adapter=")
    print("lane_id=")
    print("gpu_vendor=")
    print("os_family=")
    print("reason=missing-report")
    raise SystemExit(0)

try:
    doc = json.loads(report_path.read_text(encoding="utf-8"))
except json.JSONDecodeError:
    print("available=0")
    print("adapter=")
    print("lane_id=")
    print("gpu_vendor=")
    print("os_family=")
    print("reason=json-decode")
    raise SystemExit(0)

kind_ok = doc.get("kind") == "genesis/gpu-device-conformance-v0.1"
ok = bool(doc.get("ok", False))
backend = str(doc.get("gpu_compute_backend", "")).strip().lower()
if backend == "device-bridge":
    backend = "device-runtime"
adapter = str(doc.get("gpu_compute_adapter", "")).strip()
available = kind_ok and ok and backend == "device-runtime" and bool(adapter)
print(f"available={1 if available else 0}")
print(f"adapter={adapter}")
print(f"lane_id={str(doc.get('lane_id', '')).strip()}")
print(f"gpu_vendor={str(doc.get('gpu_vendor', '')).strip()}")
print(f"os_family={str(doc.get('os_family', '')).strip()}")
if not kind_ok:
    print("reason=kind-mismatch")
elif not ok:
    print("reason=report-not-ok")
elif backend != "device-runtime":
    print("reason=backend-not-device-runtime")
elif not adapter:
    print("reason=missing-adapter")
else:
    print("reason=available")
PY
}

run_lane_workflows() {
  local lane="$1"
  local strict_mode="$2"
  local min_free_kb="$3"
  local auto_reclaim="$4"
  local require_device="$5"
  local lane_policy="$6"
  local expected_backend="$7"
  local lane_tmp="$TMP_ROOT_BASE/$lane"
  local lane_results="$RESULTS_DIR/$lane"
  mkdir -p "$lane_results"
  printf '%s\n' "$require_device" >"$lane_results/require_device.txt"
  printf '%s\n' "$lane_policy" >"$lane_results/backend_policy.txt"
  printf '%s\n' "$expected_backend" >"$lane_results/expected_backend.txt"

  genesis_heavy_gate_preflight \
    "$ROOT_DIR" \
    "gpu-gfx-headroom-$lane" \
    "$min_free_kb" \
    "$lane_tmp" \
    "$auto_reclaim" \
    "$strict_mode"

  local workflows=(
    "agent_gpu_compute_workflow:$ROOT_DIR/examples/agent_gpu_compute_workflow/workflow.sh"
    "agent_interactive_gfx_compute_workflow:$ROOT_DIR/examples/agent_interactive_gfx_compute_workflow/workflow.sh"
  )
  local entry
  for entry in "${workflows[@]}"; do
    local wf_name="${entry%%:*}"
    local wf_path="${entry#*:}"
    local wf_dir="$lane_results/$wf_name"
    mkdir -p "$wf_dir"

    local run1_ok=1
    local run2_ok=1
    local replay1=""
    local replay2=""
    local backend1=""
    local backend2=""

    if ! TMPDIR="$lane_tmp" GENESIS_BIN="$GENESIS_BIN" GENESIS_AGENT_GPU_REQUIRE_DEVICE="$require_device" bash "$wf_path" >"$wf_dir/run1.stdout" 2>"$wf_dir/run1.stderr"; then
      run1_ok=0
      FAILURES+=("$lane/$wf_name:run1-failed")
    fi
    if [[ "$run1_ok" == "1" ]]; then
      replay1="$(
        {
          cat "$wf_dir/run1.stdout"
          cat "$wf_dir/run1.stderr"
        } | sed -n 's/.*replay=//p' | tail -n 1 | tr -d '\n'
      )"
      if [[ -z "$replay1" ]]; then
        FAILURES+=("$lane/$wf_name:run1-missing-replay")
      fi
      backend1="$(
        printf '%s\n' "$replay1" | sed -n 's/.*:backend "\([^"]*\)".*/\1/p' | tail -n 1 | tr -d '\n'
      )"
      if [[ -z "$backend1" ]]; then
        FAILURES+=("$lane/$wf_name:run1-missing-backend")
      fi
    fi

    if ! TMPDIR="$lane_tmp" GENESIS_BIN="$GENESIS_BIN" GENESIS_AGENT_GPU_REQUIRE_DEVICE="$require_device" bash "$wf_path" >"$wf_dir/run2.stdout" 2>"$wf_dir/run2.stderr"; then
      run2_ok=0
      FAILURES+=("$lane/$wf_name:run2-failed")
    fi
    if [[ "$run2_ok" == "1" ]]; then
      replay2="$(
        {
          cat "$wf_dir/run2.stdout"
          cat "$wf_dir/run2.stderr"
        } | sed -n 's/.*replay=//p' | tail -n 1 | tr -d '\n'
      )"
      if [[ -z "$replay2" ]]; then
        FAILURES+=("$lane/$wf_name:run2-missing-replay")
      fi
      backend2="$(
        printf '%s\n' "$replay2" | sed -n 's/.*:backend "\([^"]*\)".*/\1/p' | tail -n 1 | tr -d '\n'
      )"
      if [[ -z "$backend2" ]]; then
        FAILURES+=("$lane/$wf_name:run2-missing-backend")
      fi
    fi

    if [[ -n "$replay1" && -n "$replay2" && "$replay1" != "$replay2" ]]; then
      FAILURES+=("$lane/$wf_name:replay-mismatch")
    fi
    if [[ -n "$backend1" && -n "$backend2" && "$backend1" != "$backend2" ]]; then
      FAILURES+=("$lane/$wf_name:backend-mismatch")
    fi
    if [[ "$expected_backend" != "any" && -n "$backend1" && "$backend1" != "$expected_backend" ]]; then
      FAILURES+=("$lane/$wf_name:backend-policy-mismatch")
    fi

    printf '%s\n' "$replay1" >"$wf_dir/replay1.txt"
    printf '%s\n' "$replay2" >"$wf_dir/replay2.txt"
    printf '%s\n' "$backend1" >"$wf_dir/backend1.txt"
    printf '%s\n' "$backend2" >"$wf_dir/backend2.txt"
  done
}

DEVICE_REFRESH_EXIT_CODE=0
if [[ "$REQUIRE_DEVICE_LANE" != "0" && "$DEVICE_CONFORMANCE_REFRESH" == "1" ]]; then
  if ! bash "$DEVICE_CONFORMANCE_CMD"; then
    DEVICE_REFRESH_EXIT_CODE=$?
  fi
fi

DEVICE_RUNTIME_AVAILABLE=0
DEVICE_RUNTIME_ADAPTER=""
DEVICE_RUNTIME_LANE_ID=""
DEVICE_RUNTIME_VENDOR=""
DEVICE_RUNTIME_OS=""
DEVICE_RUNTIME_REASON=""
while IFS='=' read -r key value; do
  case "$key" in
    available) DEVICE_RUNTIME_AVAILABLE="$value" ;;
    adapter) DEVICE_RUNTIME_ADAPTER="$value" ;;
    lane_id) DEVICE_RUNTIME_LANE_ID="$value" ;;
    gpu_vendor) DEVICE_RUNTIME_VENDOR="$value" ;;
    os_family) DEVICE_RUNTIME_OS="$value" ;;
    reason) DEVICE_RUNTIME_REASON="$value" ;;
  esac
done < <(resolve_device_runtime_availability "$DEVICE_CONFORMANCE_REPORT")

REQUIRE_DEVICE_LANE_ACTIVE=0
case "$REQUIRE_DEVICE_LANE" in
  1)
    REQUIRE_DEVICE_LANE_ACTIVE=1
    ;;
  auto)
    if [[ "$DEVICE_RUNTIME_AVAILABLE" == "1" ]]; then
      REQUIRE_DEVICE_LANE_ACTIVE=1
    fi
    ;;
esac

if [[ "$REQUIRE_DEVICE_LANE" == "1" && "$DEVICE_RUNTIME_AVAILABLE" != "1" ]]; then
  FAILURES+=("normal/require-device-lane-unavailable")
fi

NORMAL_REQUIRE_DEVICE=0
NORMAL_BACKEND_POLICY="dev-allow-fallback"
NORMAL_EXPECTED_BACKEND="deterministic-fallback"
if [[ "$REQUIRE_DEVICE_LANE_ACTIVE" == "1" ]]; then
  NORMAL_REQUIRE_DEVICE=1
  NORMAL_BACKEND_POLICY="require-device"
  NORMAL_EXPECTED_BACKEND="device-runtime"
fi

LOW_REQUIRE_DEVICE=0
LOW_BACKEND_POLICY="allow-fallback-under-headroom"
LOW_EXPECTED_BACKEND="any"

run_lane_workflows "normal" "$NORMAL_STRICT_MODE" "$NORMAL_MIN_FREE_KB" "$NORMAL_AUTO_RECLAIM" "$NORMAL_REQUIRE_DEVICE" "$NORMAL_BACKEND_POLICY" "$NORMAL_EXPECTED_BACKEND"
run_lane_workflows "low-headroom" "$LOW_STRICT_MODE" "$LOW_MIN_FREE_KB" "$LOW_AUTO_RECLAIM" "$LOW_REQUIRE_DEVICE" "$LOW_BACKEND_POLICY" "$LOW_EXPECTED_BACKEND"

END_MS="$(python3 - <<'PY'
import time
print(int(time.time() * 1000))
PY
)"
ELAPSED_MS=$((END_MS - START_MS))

failure_args=()
if ((${#FAILURES[@]} > 0)); then
  failure_args=("${FAILURES[@]}")
fi

python3 - "$RESULTS_DIR" "$REPORT_OUT" "$HISTORY_OUT" "$ELAPSED_MS" "$BUDGET_MS" "$FREE_KB_START" "$LOW_MIN_FREE_KB" "$LOW_HEADROOM_SIMULATED" "$REQUIRE_DEVICE_LANE" "$REQUIRE_DEVICE_LANE_ACTIVE" "$DEVICE_RUNTIME_AVAILABLE" "$DEVICE_RUNTIME_ADAPTER" "$DEVICE_RUNTIME_LANE_ID" "$DEVICE_RUNTIME_VENDOR" "$DEVICE_RUNTIME_OS" "$DEVICE_RUNTIME_REASON" "$DEVICE_REFRESH_EXIT_CODE" "$DEVICE_CONFORMANCE_REPORT" "${failure_args[@]-}" <<'PY'
import datetime as dt
import json
import pathlib
import sys

results_dir = pathlib.Path(sys.argv[1])
report_out = pathlib.Path(sys.argv[2])
history_out = pathlib.Path(sys.argv[3])
elapsed_ms = int(sys.argv[4])
budget_ms = int(sys.argv[5])
free_kb_start = int(sys.argv[6])
low_min_free_kb = int(sys.argv[7])
low_headroom_simulated = sys.argv[8] == "1"
require_device_lane_mode = sys.argv[9]
require_device_lane_active = sys.argv[10] == "1"
device_runtime_available = sys.argv[11] == "1"
device_runtime_adapter = sys.argv[12]
device_runtime_lane_id = sys.argv[13]
device_runtime_vendor = sys.argv[14]
device_runtime_os = sys.argv[15]
device_runtime_reason = sys.argv[16]
device_refresh_exit_code = int(sys.argv[17])
device_conformance_report = sys.argv[18]
failures = [x for x in sys.argv[19:] if x]

lanes = {}
for lane_dir in sorted(p for p in results_dir.iterdir() if p.is_dir()):
    lane_name = lane_dir.name
    lane_policy = (lane_dir / "backend_policy.txt").read_text(encoding="utf-8").strip()
    expected_backend = (lane_dir / "expected_backend.txt").read_text(encoding="utf-8").strip()
    require_device = (lane_dir / "require_device.txt").read_text(encoding="utf-8").strip() == "1"
    workflows = []
    for wf_dir in sorted(p for p in lane_dir.iterdir() if p.is_dir()):
        replay1 = (wf_dir / "replay1.txt").read_text(encoding="utf-8").strip()
        replay2 = (wf_dir / "replay2.txt").read_text(encoding="utf-8").strip()
        backend1 = (wf_dir / "backend1.txt").read_text(encoding="utf-8").strip()
        backend2 = (wf_dir / "backend2.txt").read_text(encoding="utf-8").strip()
        run1_stdout = (wf_dir / "run1.stdout").read_text(encoding="utf-8")
        run2_stdout = (wf_dir / "run2.stdout").read_text(encoding="utf-8")
        run1_stderr = (wf_dir / "run1.stderr").read_text(encoding="utf-8")
        run2_stderr = (wf_dir / "run2.stderr").read_text(encoding="utf-8")
        wf_failures = [f for f in failures if f.startswith(f"{lane_name}/{wf_dir.name}:")]
        observed_backend = backend1 if backend1 else backend2
        workflows.append(
            {
                "workflow": wf_dir.name,
                "ok": len(wf_failures) == 0,
                "failures": wf_failures,
                "replay1": replay1,
                "replay2": replay2,
                "backend1": backend1,
                "backend2": backend2,
                "observed_backend": observed_backend,
                "run1_stdout_tail": run1_stdout[-300:],
                "run2_stdout_tail": run2_stdout[-300:],
                "run1_stderr_tail": run1_stderr[-300:],
                "run2_stderr_tail": run2_stderr[-300:],
            }
        )
    observed_backends = sorted(
        {
            str(wf.get("observed_backend", "")).strip()
            for wf in workflows
            if str(wf.get("observed_backend", "")).strip()
        }
    )
    fallback_evidence = [
        wf["workflow"]
        for wf in workflows
        if str(wf.get("observed_backend", "")).strip()
        and str(wf.get("observed_backend", "")).strip() != "device-runtime"
    ]
    lanes[lane_name] = {
        "backend_policy": lane_policy,
        "expected_backend": expected_backend,
        "require_device": require_device,
        "workflow_count": len(workflows),
        "workflow_successes": sum(1 for wf in workflows if wf["ok"]),
        "ok": all(wf["ok"] for wf in workflows),
        "observed_backends": observed_backends,
        "fallback_observed": bool(fallback_evidence),
        "fallback_evidence_workflows": fallback_evidence,
        "fallback_policy": "allow-fallback-under-headroom" if lane_name == "low-headroom" else None,
        "workflows": workflows,
    }

fail_reasons = []
if failures:
    fail_reasons.append("workflow-failures")
if elapsed_ms > budget_ms:
    fail_reasons.append("elapsed-budget")
if not low_headroom_simulated:
    fail_reasons.append("low-headroom-not-simulated")
if require_device_lane_mode == "1" and not device_runtime_available:
    fail_reasons.append("require-device-lane-unavailable")
if require_device_lane_mode == "auto" and device_runtime_available and not require_device_lane_active:
    fail_reasons.append("require-device-lane-not-activated")
normal_lane = lanes.get("normal")
if require_device_lane_active:
    if not isinstance(normal_lane, dict):
        fail_reasons.append("missing-normal-lane")
    else:
        if normal_lane.get("backend_policy") != "require-device":
            fail_reasons.append("require-device-lane-policy-mismatch")
        if normal_lane.get("expected_backend") != "device-runtime":
            fail_reasons.append("require-device-lane-expected-backend-mismatch")
low_headroom_lane = lanes.get("low-headroom")
if not isinstance(low_headroom_lane, dict):
    fail_reasons.append("missing-low-headroom-lane")
elif low_headroom_lane.get("fallback_policy") != "allow-fallback-under-headroom":
    fail_reasons.append("low-headroom-fallback-policy-missing")

ok = not fail_reasons
now_utc = dt.datetime.now(dt.timezone.utc).replace(microsecond=0).isoformat()
doc = {
    "kind": "genesis/gpu-gfx-headroom-conformance-v0.1",
    "ok": ok,
    "elapsed_ms": elapsed_ms,
    "budget_ms": budget_ms,
    "free_kb_start": free_kb_start,
    "low_headroom_min_free_kb": low_min_free_kb,
    "low_headroom_simulated": low_headroom_simulated,
    "require_device_lane_mode": require_device_lane_mode,
    "require_device_lane_active": require_device_lane_active,
    "device_runtime_available": device_runtime_available,
    "device_runtime_conformance": {
        "report_path": device_conformance_report,
        "refresh_exit_code": device_refresh_exit_code,
        "availability_reason": device_runtime_reason,
        "adapter": device_runtime_adapter or None,
        "lane_id": device_runtime_lane_id or None,
        "gpu_vendor": device_runtime_vendor or None,
        "os_family": device_runtime_os or None,
    },
    "lanes": lanes,
    "failures": failures,
    "fail_reasons": fail_reasons,
    "timestamp_utc": now_utc,
}

if report_out.is_file():
    try:
        prev = json.loads(report_out.read_text(encoding="utf-8"))
        if isinstance(prev, dict) and isinstance(prev.get("elapsed_ms"), int):
            doc["previous_elapsed_ms"] = int(prev["elapsed_ms"])
            doc["elapsed_delta_ms"] = elapsed_ms - int(prev["elapsed_ms"])
    except json.JSONDecodeError:
        pass

report_out.parent.mkdir(parents=True, exist_ok=True)
history_out.parent.mkdir(parents=True, exist_ok=True)
report_out.write_text(json.dumps(doc, indent=2, sort_keys=True) + "\n", encoding="utf-8")
with history_out.open("a", encoding="utf-8") as fh:
    fh.write(
        json.dumps(
            {
                "kind": doc["kind"],
                "ok": ok,
                "elapsed_ms": elapsed_ms,
                "budget_ms": budget_ms,
                "low_headroom_simulated": low_headroom_simulated,
                "timestamp_utc": now_utc,
            },
            sort_keys=True,
        )
        + "\n"
    )

print(
    "gpu-gfx-headroom: "
    f"report={report_out} ok={ok} elapsed_ms={elapsed_ms} budget_ms={budget_ms} "
    f"low_headroom_simulated={low_headroom_simulated}"
)
if fail_reasons:
    raise SystemExit(
        "gpu-gfx-headroom: fail reasons: " + ", ".join(fail_reasons)
    )
PY

echo "gpu-gfx-headroom: ok"
