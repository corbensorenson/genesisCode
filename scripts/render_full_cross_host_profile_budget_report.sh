#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if [[ "$#" -ne 5 ]]; then
  echo "usage: $0 <report-output> <history-output> <history-input> <strict-report-input> <wasm-report-input>" >&2
  exit 2
fi

FULL_REPORT="$1"
FULL_HISTORY="$2"
FULL_HISTORY_INPUT="$3"
STRICT_REPORT="$4"
WASM_REPORT="$5"
FULL_BASELINE_HISTORY="${GENESIS_FULL_CROSS_HOST_BASELINE_HISTORY:-policies/perf/full_cross_host_profile_seed_history.jsonl}"
FULL_BUDGET_MS="${GENESIS_FULL_CROSS_HOST_BUDGET_MS:-720000}"
FULL_MIN_HISTORY="${GENESIS_FULL_CROSS_HOST_MIN_HISTORY:-5}"

[[ "$FULL_BUDGET_MS" =~ ^[0-9]+$ && "$FULL_BUDGET_MS" -gt 0 ]] || {
  echo "full-cross-host-budget: GENESIS_FULL_CROSS_HOST_BUDGET_MS must be a positive integer" >&2
  exit 2
}
[[ "$FULL_MIN_HISTORY" =~ ^[0-9]+$ && "$FULL_MIN_HISTORY" -gt 0 ]] || {
  echo "full-cross-host-budget: GENESIS_FULL_CROSS_HOST_MIN_HISTORY must be a positive integer" >&2
  exit 2
}
[[ -f "$FULL_BASELINE_HISTORY" ]] || {
  echo "full-cross-host-budget: baseline history file missing: $FULL_BASELINE_HISTORY" >&2
  exit 1
}

[[ -f "$STRICT_REPORT" ]] || {
  echo "full-cross-host-budget: strict-golden report missing: $STRICT_REPORT" >&2
  echo "full-cross-host-budget: produce it with: bash scripts/selfhost_strict_golden.sh" >&2
  exit 1
}
[[ -f "$WASM_REPORT" ]] || {
  echo "full-cross-host-budget: wasm cross-host report missing: $WASM_REPORT" >&2
  echo 'full-cross-host-budget: produce it with: wasm_js_path="$(bash scripts/wasm_bindgen_node.sh | tail -n 1)" && node scripts/wasm_cross_host_determinism.mjs "$wasm_js_path"' >&2
  exit 1
}

read -r STRICT_ELAPSED_MS WASM_ELAPSED_MS < <(
  python3 - "$STRICT_REPORT" "$WASM_REPORT" <<'PY'
import json
import pathlib
import sys

strict_path = pathlib.Path(sys.argv[1])
wasm_path = pathlib.Path(sys.argv[2])

def load_elapsed(path: pathlib.Path, profile: str) -> int:
    doc = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(doc, dict):
        raise SystemExit(f"full-cross-host-budget: report must be an object: {path}")
    if doc.get("kind") != "genesis/test-profile-runtime-v0.1":
        raise SystemExit(f"full-cross-host-budget: unexpected kind in {path}: {doc.get('kind')!r}")
    if doc.get("profile") != profile:
        raise SystemExit(
            f"full-cross-host-budget: expected profile {profile!r} in {path}, got {doc.get('profile')!r}"
        )
    elapsed = doc.get("elapsed_ms")
    if not isinstance(elapsed, int) or elapsed <= 0:
        raise SystemExit(f"full-cross-host-budget: invalid elapsed_ms in {path}")
    return elapsed

strict_elapsed = load_elapsed(strict_path, "strict-golden")
wasm_elapsed = load_elapsed(wasm_path, "wasm-cross-host")
print(f"{strict_elapsed} {wasm_elapsed}")
PY
)

TOTAL_ELAPSED_MS=$((STRICT_ELAPSED_MS + WASM_ELAPSED_MS))
EXTRA_JSON="$(python3 - "$STRICT_REPORT" "$WASM_REPORT" "$STRICT_ELAPSED_MS" "$WASM_ELAPSED_MS" <<'PY'
import hashlib
import json
import pathlib
import sys

strict_path = pathlib.Path(sys.argv[1])
wasm_path = pathlib.Path(sys.argv[2])
print(json.dumps({
    "command": "strict-golden + wasm-cross-host",
    "strict_elapsed_ms": int(sys.argv[3]),
    "wasm_cross_host_elapsed_ms": int(sys.argv[4]),
    "strict_report": "strict-golden",
    "strict_report_sha256": hashlib.sha256(strict_path.read_bytes()).hexdigest(),
    "wasm_report": "wasm-cross-host",
    "wasm_report_sha256": hashlib.sha256(wasm_path.read_bytes()).hexdigest(),
}, sort_keys=True, separators=(",", ":")))
PY
)"

EFFECTIVE_BASELINE_HISTORY="$FULL_BASELINE_HISTORY"
MERGED_BASELINE_HISTORY=""
if [[ "$FULL_HISTORY_INPUT" != "$FULL_HISTORY" && -f "$FULL_HISTORY_INPUT" ]]; then
  MERGED_BASELINE_HISTORY="$(mktemp)"
  cat "$FULL_BASELINE_HISTORY" "$FULL_HISTORY_INPUT" >"$MERGED_BASELINE_HISTORY"
  EFFECTIVE_BASELINE_HISTORY="$MERGED_BASELINE_HISTORY"
fi
cleanup() {
  [[ -z "$MERGED_BASELINE_HISTORY" ]] || rm -f "$MERGED_BASELINE_HISTORY"
}
trap cleanup EXIT

set +e
python3 "$ROOT_DIR/scripts/lib/profile_runtime_budget.py" \
  --profile full-cross-host \
  --kind genesis/test-profile-runtime-v0.1 \
  --report "$FULL_REPORT" \
  --history "$FULL_HISTORY" \
  --baseline-history "$EFFECTIVE_BASELINE_HISTORY" \
  --require-min-history \
  --elapsed-ms "$TOTAL_ELAPSED_MS" \
  --budget-ms "$FULL_BUDGET_MS" \
  --min-history "$FULL_MIN_HISTORY" \
  --extra-json "$EXTRA_JSON"
budget_status=$?
set -e

python3 - "$FULL_REPORT" "$FULL_HISTORY" "$EFFECTIVE_BASELINE_HISTORY" <<'PY'
import hashlib
import json
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
history_path = pathlib.Path(sys.argv[2])
baseline_path = pathlib.Path(sys.argv[3])
doc = json.loads(path.read_text(encoding="utf-8"))
doc["history_file"] = "full-cross-host-history"
doc["history_sha256"] = hashlib.sha256(history_path.read_bytes()).hexdigest()
doc["baseline_history_file"] = "full-cross-host-baseline"
doc["baseline_history_sha256"] = hashlib.sha256(baseline_path.read_bytes()).hexdigest()
path.write_text(json.dumps(doc, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY

if [[ "$budget_status" -ne 0 ]]; then
  exit "$budget_status"
fi

echo "full-cross-host-budget: ok total_elapsed_ms=$TOTAL_ELAPSED_MS budget_ms=$FULL_BUDGET_MS"
