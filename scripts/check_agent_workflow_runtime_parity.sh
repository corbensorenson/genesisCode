#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "check-agent-workflow-runtime-parity" \
  ".genesis/build/cargo" \
  "GENESIS_CHECK_AGENT_WORKFLOW_RUNTIME_PARITY_CARGO_TARGET_DIR"

DEFAULT_DEBUG_DIR="$CARGO_TARGET_DIR/debug"
NATIVE_BIN="${GENESIS_AGENT_PARITY_NATIVE_BIN:-$DEFAULT_DEBUG_DIR/genesis}"
WASI_BIN="${GENESIS_AGENT_PARITY_WASI_BIN:-$DEFAULT_DEBUG_DIR/genesis_wasi}"
GAUNTLET_PROFILE="${GENESIS_AGENT_PARITY_GAUNTLET_PROFILE:-prepush-standard}"
REPORT_PATH="${GENESIS_AGENT_PARITY_REPORT:-.genesis/perf/agent_workflow_runtime_parity_report.json}"
HISTORY_PATH="${GENESIS_AGENT_PARITY_HISTORY:-.genesis/perf/agent_workflow_runtime_parity_history.jsonl}"
NATIVE_REPORT="${GENESIS_AGENT_PARITY_NATIVE_REPORT:-.genesis/perf/agent_capability_gauntlet_native_report.json}"
WASI_REPORT="${GENESIS_AGENT_PARITY_WASI_REPORT:-.genesis/perf/agent_capability_gauntlet_wasi_report.json}"
NATIVE_HISTORY="${GENESIS_AGENT_PARITY_NATIVE_HISTORY:-.genesis/perf/agent_capability_gauntlet_native_history.jsonl}"
WASI_HISTORY="${GENESIS_AGENT_PARITY_WASI_HISTORY:-.genesis/perf/agent_capability_gauntlet_wasi_history.jsonl}"
GENERATIVE_REPORT="${GENESIS_AGENT_PARITY_GENERATIVE_REPORT:-.genesis/perf/agent_generative_workloads_parity_report.json}"
GENERATIVE_HISTORY="${GENESIS_AGENT_PARITY_GENERATIVE_HISTORY:-.genesis/perf/agent_generative_workloads_parity_history.jsonl}"
GENERATIVE_SEED="${GENESIS_AGENT_PARITY_GENERATIVE_SEED:-genesis-agent-generative-parity-v1}"
BUDGET_MS="${GENESIS_AGENT_PARITY_BUDGET_MS:-900000}"
P95_MIN_SAMPLES="${GENESIS_AGENT_PARITY_P95_MIN_SAMPLES:-8}"

if [[ ! "$BUDGET_MS" =~ ^[0-9]+$ || "$BUDGET_MS" -le 0 ]]; then
  echo "agent-workflow-runtime-parity: GENESIS_AGENT_PARITY_BUDGET_MS must be a positive integer" >&2
  exit 2
fi
if [[ ! "$P95_MIN_SAMPLES" =~ ^[0-9]+$ || "$P95_MIN_SAMPLES" -le 0 ]]; then
  echo "agent-workflow-runtime-parity: GENESIS_AGENT_PARITY_P95_MIN_SAMPLES must be a positive integer" >&2
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
  GENESIS_BIN="$lane_bin" \
  GENESIS_AGENT_GAUNTLET_PROFILE="$GAUNTLET_PROFILE" \
  GENESIS_AGENT_GAUNTLET_RUNTIME_PROFILE="$runtime_profile" \
  GENESIS_AGENT_GAUNTLET_REPORT="$lane_report" \
  GENESIS_AGENT_GAUNTLET_HISTORY="$lane_history" \
  bash scripts/check_agent_reference_workflows.sh
}

run_gauntlet_lane "$NATIVE_BIN" "native" "$NATIVE_REPORT" "$NATIVE_HISTORY" &
native_pid=$!
run_gauntlet_lane "$WASI_BIN" "wasi-wasm-host-bridge" "$WASI_REPORT" "$WASI_HISTORY" &
wasi_pid=$!

lane_failures=0
if ! wait "$native_pid"; then
  echo "agent-workflow-runtime-parity: native lane failed" >&2
  lane_failures=1
fi
if ! wait "$wasi_pid"; then
  echo "agent-workflow-runtime-parity: wasi lane failed" >&2
  lane_failures=1
fi
if [[ "$lane_failures" -ne 0 ]]; then
  exit 1
fi

GENESIS_AGENT_GENERATIVE_PRIMARY_REPORT="$NATIVE_REPORT" \
GENESIS_AGENT_GENERATIVE_SECONDARY_REPORT="$WASI_REPORT" \
GENESIS_AGENT_GENERATIVE_REQUIRE_SECONDARY=1 \
GENESIS_AGENT_GENERATIVE_REPORT="$GENERATIVE_REPORT" \
GENESIS_AGENT_GENERATIVE_HISTORY="$GENERATIVE_HISTORY" \
GENESIS_AGENT_GENERATIVE_SEED="$GENERATIVE_SEED" \
bash scripts/check_agent_generative_workloads.sh

end_ns="$(python3 - <<'PY'
import time
print(time.time_ns())
PY
)"
elapsed_ms="$(( (end_ns - start_ns) / 1000000 ))"

python3 - "$NATIVE_REPORT" "$WASI_REPORT" "$REPORT_PATH" "$HISTORY_PATH" "$elapsed_ms" "$BUDGET_MS" "$GAUNTLET_PROFILE" "$NATIVE_BIN" "$WASI_BIN" "$P95_MIN_SAMPLES" <<'PY'
import datetime as dt
import json
import math
import pathlib
import sys

native_report_path = pathlib.Path(sys.argv[1])
wasi_report_path = pathlib.Path(sys.argv[2])
report_path = pathlib.Path(sys.argv[3])
history_path = pathlib.Path(sys.argv[4])
elapsed_ms = int(sys.argv[5])
budget_ms = int(sys.argv[6])
gauntlet_profile = sys.argv[7]
native_bin = sys.argv[8]
wasi_bin = sys.argv[9]
p95_min_samples = int(sys.argv[10])

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

elapsed_history = load_elapsed_history(history_path, gauntlet_profile, budget_ms)
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
    "native_bin": native_bin,
    "wasi_bin": wasi_bin,
    "native_report": str(native_report_path),
    "wasi_report": str(wasi_report_path),
    "workflow_count": len(all_names),
    "domain_count": len(all_domains),
    "elapsed_ms": elapsed_ms,
    "budget_ms": budget_ms,
    "history_samples": history_samples,
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
