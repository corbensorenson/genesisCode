#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if [[ "$#" -ne 14 ]]; then
  echo "usage: $0 <report-output> <history-output> <history-input> <native-report-output> <native-history-output> <native-report-input> <native-history-input> <wasi-report-output> <wasi-history-output> <wasi-report-input> <wasi-history-input> <generative-report-output> <generative-history-output> <generative-history-input>" >&2
  exit 2
fi

REPORT_PATH="$1"
HISTORY_PATH="$2"
HISTORY_INPUT_PATH="$3"
NATIVE_REPORT_OUT="$4"
NATIVE_HISTORY_OUT="$5"
NATIVE_REPORT_INPUT="$6"
NATIVE_HISTORY_INPUT="$7"
WASI_REPORT_OUT="$8"
WASI_HISTORY_OUT="$9"
WASI_REPORT_INPUT="${10}"
WASI_HISTORY_INPUT="${11}"
GENERATIVE_REPORT="${12}"
GENERATIVE_HISTORY="${13}"
GENERATIVE_HISTORY_INPUT="${14}"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "check-agent-workflow-runtime-parity" \
  root-host

DEFAULT_DEBUG_DIR="$CARGO_TARGET_DIR/debug"
NATIVE_BIN="${GENESIS_AGENT_PARITY_NATIVE_BIN:-$DEFAULT_DEBUG_DIR/genesis}"
WASI_BIN="${GENESIS_AGENT_PARITY_WASI_BIN:-$DEFAULT_DEBUG_DIR/genesis_wasi}"
GAUNTLET_PROFILE="${GENESIS_AGENT_PARITY_GAUNTLET_PROFILE:-prepush-standard}"
GENERATIVE_SEED="${GENESIS_AGENT_PARITY_GENERATIVE_SEED:-genesis-agent-generative-parity-v1}"
GENERATIVE_CASE_COUNT="${GENESIS_AGENT_PARITY_GENERATIVE_CASE_COUNT:-40}"
BUDGET_MS="${GENESIS_AGENT_PARITY_BUDGET_MS:-900000}"
P95_MIN_SAMPLES="${GENESIS_AGENT_PARITY_P95_MIN_SAMPLES:-8}"
INPUT_MAX_AGE_SEC="${GENESIS_AGENT_PARITY_INPUT_MAX_AGE_SEC:-21600}"
REUSE_REPORTS="${GENESIS_AGENT_PARITY_REUSE_REPORTS:-1}"
PARITY_TMP_ROOT="$(mktemp -d)"
cleanup() {
  rm -rf "$PARITY_TMP_ROOT"
}
trap cleanup EXIT

if [[ ! "$BUDGET_MS" =~ ^[0-9]+$ || "$BUDGET_MS" -le 0 ]]; then
  echo "agent-workflow-runtime-parity: GENESIS_AGENT_PARITY_BUDGET_MS must be a positive integer" >&2
  exit 2
fi
if [[ ! "$P95_MIN_SAMPLES" =~ ^[0-9]+$ || "$P95_MIN_SAMPLES" -le 0 ]]; then
  echo "agent-workflow-runtime-parity: GENESIS_AGENT_PARITY_P95_MIN_SAMPLES must be a positive integer" >&2
  exit 2
fi
if [[ ! "$INPUT_MAX_AGE_SEC" =~ ^[0-9]+$ ]]; then
  echo "agent-workflow-runtime-parity: GENESIS_AGENT_PARITY_INPUT_MAX_AGE_SEC must be a non-negative integer" >&2
  exit 2
fi
if [[ "$REUSE_REPORTS" != "0" && "$REUSE_REPORTS" != "1" ]]; then
  echo "agent-workflow-runtime-parity: GENESIS_AGENT_PARITY_REUSE_REPORTS must be 0 or 1" >&2
  exit 2
fi
if [[ ! "$GENERATIVE_CASE_COUNT" =~ ^[0-9]+$ || "$GENERATIVE_CASE_COUNT" -le 0 ]]; then
  echo "agent-workflow-runtime-parity: GENESIS_AGENT_PARITY_GENERATIVE_CASE_COUNT must be a positive integer" >&2
  exit 2
fi

if [[ ! -x "$NATIVE_BIN" ]]; then
  cargo build -p gc_cli >/dev/null
fi
if [[ ! -x "$WASI_BIN" ]]; then
  cargo build -p gc_wasi_cli >/dev/null
fi

start_ns="$(python3 - <<'PY'
import time
print(time.time_ns())
PY
)"

run_gauntlet_lane() {
  local lane_bin="$1"
  local runtime_profile="$2"
  local lane_report="$3"
  local lane_history="$4"
  local lane_history_input="$5"
  local lane_tmp_root="$6"
  GENESIS_BIN="$lane_bin" \
  GENESIS_AGENT_GAUNTLET_PROFILE="$GAUNTLET_PROFILE" \
  GENESIS_AGENT_GAUNTLET_RUNTIME_PROFILE="$runtime_profile" \
  GENESIS_AGENT_REFERENCE_WORKFLOWS_TMPDIR="$lane_tmp_root" \
  bash scripts/render_agent_reference_workflows_report.sh \
    "$lane_report" \
    "$lane_history" \
    "$lane_history_input"
}

lane_source="fresh-run"
reuse_detail=""
can_reuse_reports=0
if [[ "$REUSE_REPORTS" == "1" ]]; then
  if reuse_detail="$(python3 - "$NATIVE_REPORT_INPUT" "$WASI_REPORT_INPUT" "$GAUNTLET_PROFILE" "$INPUT_MAX_AGE_SEC" <<'PY'
import json
import pathlib
import sys
import time

native_path = pathlib.Path(sys.argv[1])
wasi_path = pathlib.Path(sys.argv[2])
expected_profile = sys.argv[3]
max_age_sec = int(sys.argv[4])

expected_kind = "genesis/agent-capability-gauntlet-v0.1"
expected_native_runtime = "native"
expected_wasi_runtime = "wasi-wasm-host-bridge"

def read_doc(path: pathlib.Path, label: str, expected_runtime: str) -> dict:
    if not path.is_file():
        raise SystemExit(f"{label}:missing")
    age = max(0, int(time.time() - path.stat().st_mtime))
    if max_age_sec > 0 and age > max_age_sec:
        raise SystemExit(f"{label}:stale age={age}s max={max_age_sec}s")
    doc = json.loads(path.read_text(encoding="utf-8"))
    if doc.get("kind") != expected_kind:
        raise SystemExit(f"{label}:kind={doc.get('kind')!r}")
    if doc.get("profile") != expected_profile:
        raise SystemExit(
            f"{label}:profile={doc.get('profile')!r} expected={expected_profile!r}"
        )
    if doc.get("runtime_profile") != expected_runtime:
        raise SystemExit(
            f"{label}:runtime_profile={doc.get('runtime_profile')!r} expected={expected_runtime!r}"
        )
    if doc.get("ok") is not True:
        raise SystemExit(f"{label}:ok=false")
    workflow_count = int(doc.get("workflow_count", 0))
    if workflow_count <= 0:
        raise SystemExit(f"{label}:workflow_count={workflow_count}")
    return {"age_sec": age, "workflow_count": workflow_count}

native = read_doc(native_path, "native-report", expected_native_runtime)
wasi = read_doc(wasi_path, "wasi-report", expected_wasi_runtime)
print(
    "reuse-native-wasi "
    f"native_age_sec={native['age_sec']} "
    f"wasi_age_sec={wasi['age_sec']} "
    f"native_workflows={native['workflow_count']} "
    f"wasi_workflows={wasi['workflow_count']}"
)
PY
  )"; then
    can_reuse_reports=1
  else
    echo "agent-workflow-runtime-parity: report reuse disabled ($reuse_detail); running fresh lanes" >&2
  fi
fi

if [[ "$can_reuse_reports" -eq 1 ]]; then
  lane_source="reused-reports"
  NATIVE_REPORT="$NATIVE_REPORT_INPUT"
  WASI_REPORT="$WASI_REPORT_INPUT"
  echo "agent-workflow-runtime-parity: reusing existing native+wasi gauntlet reports ($reuse_detail)"
else
  NATIVE_REPORT="$NATIVE_REPORT_OUT"
  WASI_REPORT="$WASI_REPORT_OUT"
  lane_failures=0
  # Parity is a semantic comparison, not a contention benchmark. Sequential
  # lanes prevent one runtime's load from changing the other's timing verdict;
  # dedicated stress gates own concurrent-load coverage.
  if ! run_gauntlet_lane \
    "$NATIVE_BIN" \
    "native" \
    "$NATIVE_REPORT_OUT" \
    "$NATIVE_HISTORY_OUT" \
    "$NATIVE_HISTORY_INPUT" \
    "$PARITY_TMP_ROOT/native"; then
    echo "agent-workflow-runtime-parity: native lane failed" >&2
    lane_failures=1
  fi
  if ! run_gauntlet_lane \
    "$WASI_BIN" \
    "wasi-wasm-host-bridge" \
    "$WASI_REPORT_OUT" \
    "$WASI_HISTORY_OUT" \
    "$WASI_HISTORY_INPUT" \
    "$PARITY_TMP_ROOT/wasi"; then
    echo "agent-workflow-runtime-parity: wasi lane failed" >&2
    lane_failures=1
  fi
  if [[ "$lane_failures" -ne 0 ]]; then
    exit 1
  fi
fi

GENESIS_AGENT_GENERATIVE_PRIMARY_REPORT="$NATIVE_REPORT" \
GENESIS_AGENT_GENERATIVE_SECONDARY_REPORT="$WASI_REPORT" \
GENESIS_AGENT_GENERATIVE_REQUIRE_SECONDARY=1 \
GENESIS_AGENT_GENERATIVE_REQUIRE_MIN_HISTORY=0 \
GENESIS_AGENT_GENERATIVE_CASE_COUNT="$GENERATIVE_CASE_COUNT" \
GENESIS_AGENT_GENERATIVE_SEED="$GENERATIVE_SEED" \
bash scripts/render_agent_generative_workloads_report.sh \
  "$GENERATIVE_REPORT" \
  "$GENERATIVE_HISTORY" \
  "$GENERATIVE_HISTORY_INPUT" \
  "$NATIVE_REPORT" \
  "$WASI_REPORT"

end_ns="$(python3 - <<'PY'
import time
print(time.time_ns())
PY
)"
elapsed_ms="$(( (end_ns - start_ns) / 1000000 ))"

python3 - "$NATIVE_REPORT" "$WASI_REPORT" "$REPORT_PATH" "$HISTORY_PATH" "$HISTORY_INPUT_PATH" "$elapsed_ms" "$BUDGET_MS" "$GAUNTLET_PROFILE" "$NATIVE_BIN" "$WASI_BIN" "$P95_MIN_SAMPLES" "$lane_source" <<'PY'
import datetime as dt
import json
import math
import pathlib
import sys

native_report_path = pathlib.Path(sys.argv[1])
wasi_report_path = pathlib.Path(sys.argv[2])
report_path = pathlib.Path(sys.argv[3])
history_path = pathlib.Path(sys.argv[4])
history_input_path = pathlib.Path(sys.argv[5])
elapsed_ms = int(sys.argv[6])
budget_ms = int(sys.argv[7])
gauntlet_profile = sys.argv[8]
native_bin = sys.argv[9]
wasi_bin = sys.argv[10]
p95_min_samples = int(sys.argv[11])
lane_source = sys.argv[12]

native = json.loads(native_report_path.read_text(encoding="utf-8"))
wasi = json.loads(wasi_report_path.read_text(encoding="utf-8"))

expected_kind = "genesis/agent-capability-gauntlet-v0.1"
if native.get("kind") != expected_kind:
    raise SystemExit(f"agent-workflow-runtime-parity: unexpected native gauntlet kind: {native.get('kind')!r}")
if wasi.get("kind") != expected_kind:
    raise SystemExit(f"agent-workflow-runtime-parity: unexpected wasi gauntlet kind: {wasi.get('kind')!r}")

if p95_min_samples < 1:
    raise SystemExit("agent-workflow-runtime-parity: p95_min_samples must be >= 1")

def p95(values: list[int]) -> int:
    idx = max(0, math.ceil(0.95 * len(values)) - 1)
    return sorted(values)[idx]

def load_elapsed_history(path: pathlib.Path, expected_profile: str, expected_budget_ms: int) -> list[int]:
    if not path.exists():
        return []
    samples: list[int] = []
    for raw_line in path.read_text(encoding="utf-8").splitlines():
        line = raw_line.strip()
        if not line:
            continue
        try:
            row = json.loads(line)
        except json.JSONDecodeError:
            continue
        if not isinstance(row, dict):
            continue
        if row.get("kind") != "genesis/agent-workflow-runtime-parity-v0.1":
            continue
        if row.get("gauntlet_profile") != expected_profile:
            continue
        row_budget = row.get("budget_ms")
        row_elapsed = row.get("elapsed_ms")
        if not isinstance(row_budget, int) or not isinstance(row_elapsed, int):
            continue
        if row_budget != expected_budget_ms:
            continue
        if row_elapsed <= 0:
            continue
        samples.append(row_elapsed)
    return samples

native_wf = {wf["name"]: wf for wf in native.get("workflows", [])}
wasi_wf = {wf["name"]: wf for wf in wasi.get("workflows", [])}
all_names = sorted(set(native_wf) | set(wasi_wf))

missing_native = [name for name in all_names if name not in native_wf]
missing_wasi = [name for name in all_names if name not in wasi_wf]

workflow_mismatches = []
for name in all_names:
    if name in missing_native or name in missing_wasi:
        continue
    n = native_wf[name]
    w = wasi_wf[name]
    n_parity_hash = n.get("replay_hash_normalized") or n.get("replay_hash")
    w_parity_hash = w.get("replay_hash_normalized") or w.get("replay_hash")
    mismatch = {
        "name": name,
        "native_ok": bool(n.get("ok")),
        "wasi_ok": bool(w.get("ok")),
        "native_replay_hash": n.get("replay_hash"),
        "wasi_replay_hash": w.get("replay_hash"),
        "native_parity_hash": n_parity_hash,
        "wasi_parity_hash": w_parity_hash,
        "parity_hash_equal": n_parity_hash == w_parity_hash,
    }
    if not mismatch["native_ok"] or not mismatch["wasi_ok"] or not mismatch["parity_hash_equal"]:
        workflow_mismatches.append(mismatch)

native_domains = {d["domain"]: d for d in native.get("domains", [])}
wasi_domains = {d["domain"]: d for d in wasi.get("domains", [])}
all_domains = sorted(set(native_domains) | set(wasi_domains))
domain_mismatches = []
for domain in all_domains:
    nd = native_domains.get(domain)
    wd = wasi_domains.get(domain)
    if nd is None or wd is None:
        domain_mismatches.append(
            {
                "domain": domain,
                "native": nd,
                "wasi": wd,
                "reason": "missing",
            }
        )
        continue
    if nd.get("successes") != wd.get("successes") or bool(nd.get("ok")) != bool(wd.get("ok")):
        domain_mismatches.append(
            {
                "domain": domain,
                "native_successes": nd.get("successes"),
                "wasi_successes": wd.get("successes"),
                "native_ok": bool(nd.get("ok")),
                "wasi_ok": bool(wd.get("ok")),
                "reason": "count-or-status-mismatch",
            }
        )

history_sources = [history_input_path]
if history_path not in history_sources:
    history_sources.append(history_path)
elapsed_history = []
for source in history_sources:
    elapsed_history.extend(load_elapsed_history(source, gauntlet_profile, budget_ms))
elapsed_samples = elapsed_history + [elapsed_ms]
history_samples = len(elapsed_samples)
history_p95_ms = p95(elapsed_samples)
history_p95_enforced = history_samples >= p95_min_samples
history_p95_ok = (not history_p95_enforced) or (history_p95_ms <= budget_ms)
elapsed_budget_ok = elapsed_ms <= budget_ms

fail_reasons = []
if missing_native:
    fail_reasons.append("missing-native-workflows")
if missing_wasi:
    fail_reasons.append("missing-wasi-workflows")
if workflow_mismatches:
    fail_reasons.append("workflow-parity-mismatch")
if domain_mismatches:
    fail_reasons.append("domain-parity-mismatch")
if not elapsed_budget_ok:
    fail_reasons.append("elapsed-budget")
if not history_p95_ok:
    fail_reasons.append("history-p95-budget")

ok = (
    not missing_native
    and not missing_wasi
    and not workflow_mismatches
    and not domain_mismatches
    and elapsed_budget_ok
    and history_p95_ok
)

report = {
    "kind": "genesis/agent-workflow-runtime-parity-v0.1",
    "ok": ok,
    "gauntlet_profile": gauntlet_profile,
    "lane_source": lane_source,
    "native_bin": native_bin,
    "wasi_bin": wasi_bin,
    "native_report": str(native_report_path),
    "wasi_report": str(wasi_report_path),
    "workflow_count": len(all_names),
    "domain_count": len(all_domains),
    "elapsed_ms": elapsed_ms,
    "budget_ms": budget_ms,
    "history_samples": history_samples,
    "history_input": str(history_input_path),
    "history_p95_ms": history_p95_ms,
    "history_p95_enforced": history_p95_enforced,
    "history_p95_ok": history_p95_ok,
    "p95_min_samples": p95_min_samples,
    "fail_reasons": fail_reasons,
    "missing_native_workflows": missing_native,
    "missing_wasi_workflows": missing_wasi,
    "workflow_mismatches": workflow_mismatches,
    "domain_mismatches": domain_mismatches,
    "timestamp_utc": dt.datetime.now(dt.timezone.utc).replace(microsecond=0).isoformat(),
}

report_path.parent.mkdir(parents=True, exist_ok=True)
report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")

history_path.parent.mkdir(parents=True, exist_ok=True)
with history_path.open("a", encoding="utf-8") as h:
    h.write(json.dumps(report, sort_keys=True) + "\n")

print(
    "agent-workflow-runtime-parity: "
    f"report={report_path} ok={ok} workflows={len(all_names)} "
    f"mismatches={len(workflow_mismatches)} domains={len(all_domains)}"
)

if not ok:
    if fail_reasons:
        print(
            "agent-workflow-runtime-parity: fail reasons: "
            + ", ".join(fail_reasons),
            file=sys.stderr,
        )
    if not elapsed_budget_ok:
        print(
            "agent-workflow-runtime-parity: elapsed budget exceeded "
            f"({elapsed_ms}ms > {budget_ms}ms)",
            file=sys.stderr,
        )
    if not history_p95_ok:
        print(
            "agent-workflow-runtime-parity: history p95 budget exceeded "
            f"({history_p95_ms}ms > {budget_ms}ms with {history_samples} samples)",
            file=sys.stderr,
        )
    if missing_native:
        print(
            "agent-workflow-runtime-parity: missing native workflows: " + ", ".join(missing_native),
            file=sys.stderr,
        )
    if missing_wasi:
        print(
            "agent-workflow-runtime-parity: missing wasi workflows: " + ", ".join(missing_wasi),
            file=sys.stderr,
        )
    if workflow_mismatches:
        print(
            "agent-workflow-runtime-parity: workflow replay mismatches: "
            + ", ".join(m["name"] for m in workflow_mismatches),
            file=sys.stderr,
        )
    if domain_mismatches:
        print(
            "agent-workflow-runtime-parity: domain mismatches: "
            + ", ".join(m["domain"] for m in domain_mismatches),
            file=sys.stderr,
        )
    raise SystemExit(1)
PY

echo "agent-workflow-runtime-parity: ok"
