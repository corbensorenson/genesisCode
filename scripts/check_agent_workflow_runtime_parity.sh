#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

NATIVE_BIN="${GENESIS_AGENT_PARITY_NATIVE_BIN:-$ROOT_DIR/target/debug/genesis}"
WASI_BIN="${GENESIS_AGENT_PARITY_WASI_BIN:-$ROOT_DIR/target/debug/genesis_wasi}"
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

if [[ ! "$BUDGET_MS" =~ ^[0-9]+$ || "$BUDGET_MS" -le 0 ]]; then
  echo "agent-workflow-runtime-parity: GENESIS_AGENT_PARITY_BUDGET_MS must be a positive integer" >&2
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

GENESIS_BIN="$NATIVE_BIN" \
GENESIS_AGENT_GAUNTLET_PROFILE="$GAUNTLET_PROFILE" \
GENESIS_AGENT_GAUNTLET_RUNTIME_PROFILE="native" \
GENESIS_AGENT_GAUNTLET_REPORT="$NATIVE_REPORT" \
GENESIS_AGENT_GAUNTLET_HISTORY="$NATIVE_HISTORY" \
bash scripts/check_agent_reference_workflows.sh

GENESIS_BIN="$WASI_BIN" \
GENESIS_AGENT_GAUNTLET_PROFILE="$GAUNTLET_PROFILE" \
GENESIS_AGENT_GAUNTLET_RUNTIME_PROFILE="wasi-wasm-host-bridge" \
GENESIS_AGENT_GAUNTLET_REPORT="$WASI_REPORT" \
GENESIS_AGENT_GAUNTLET_HISTORY="$WASI_HISTORY" \
bash scripts/check_agent_reference_workflows.sh

GENESIS_AGENT_GENERATIVE_PRIMARY_REPORT="$NATIVE_REPORT" \
GENESIS_AGENT_GENERATIVE_SECONDARY_REPORT="$WASI_REPORT" \
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

python3 - "$NATIVE_REPORT" "$WASI_REPORT" "$REPORT_PATH" "$HISTORY_PATH" "$elapsed_ms" "$BUDGET_MS" "$GAUNTLET_PROFILE" "$NATIVE_BIN" "$WASI_BIN" <<'PY'
import datetime as dt
import json
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

native = json.loads(native_report_path.read_text(encoding="utf-8"))
wasi = json.loads(wasi_report_path.read_text(encoding="utf-8"))

expected_kind = "genesis/agent-capability-gauntlet-v0.1"
if native.get("kind") != expected_kind:
    raise SystemExit(f"agent-workflow-runtime-parity: unexpected native gauntlet kind: {native.get('kind')!r}")
if wasi.get("kind") != expected_kind:
    raise SystemExit(f"agent-workflow-runtime-parity: unexpected wasi gauntlet kind: {wasi.get('kind')!r}")

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

ok = (
    not missing_native
    and not missing_wasi
    and not workflow_mismatches
    and not domain_mismatches
    and elapsed_ms <= budget_ms
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
    if elapsed_ms > budget_ms:
        print(
            "agent-workflow-runtime-parity: elapsed budget exceeded "
            f"({elapsed_ms}ms > {budget_ms}ms)",
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
