#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

STRICT_REPORT="${GENESIS_STRICT_GOLDEN_PROFILE_REPORT:-.genesis/perf/strict_golden_profile_report.json}"
WASM_REPORT="${GENESIS_WASM_CROSS_HOST_PROFILE_REPORT:-.genesis/perf/wasm_cross_host_profile_report.json}"
FULL_REPORT="${GENESIS_FULL_CROSS_HOST_PROFILE_REPORT:-.genesis/perf/full_cross_host_profile_report.json}"
FULL_HISTORY="${GENESIS_FULL_CROSS_HOST_PROFILE_HISTORY:-.genesis/perf/full_cross_host_profile_history.jsonl}"
FULL_BUDGET_MS="${GENESIS_FULL_CROSS_HOST_BUDGET_MS:-1200000}"
FULL_MIN_HISTORY="${GENESIS_FULL_CROSS_HOST_MIN_HISTORY:-5}"

[[ -f "$STRICT_REPORT" ]] || {
  echo "full-cross-host-budget: missing strict-golden report: $STRICT_REPORT" >&2
  exit 1
}
[[ -f "$WASM_REPORT" ]] || {
  echo "full-cross-host-budget: missing wasm cross-host report: $WASM_REPORT" >&2
  exit 1
}
[[ "$FULL_BUDGET_MS" =~ ^[0-9]+$ && "$FULL_BUDGET_MS" -gt 0 ]] || {
  echo "full-cross-host-budget: GENESIS_FULL_CROSS_HOST_BUDGET_MS must be a positive integer" >&2
  exit 2
}
[[ "$FULL_MIN_HISTORY" =~ ^[0-9]+$ && "$FULL_MIN_HISTORY" -gt 0 ]] || {
  echo "full-cross-host-budget: GENESIS_FULL_CROSS_HOST_MIN_HISTORY must be a positive integer" >&2
  exit 2
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
python3 "$ROOT_DIR/scripts/lib/profile_runtime_budget.py" \
  --profile full-cross-host \
  --kind genesis/test-profile-runtime-v0.1 \
  --report "$FULL_REPORT" \
  --history "$FULL_HISTORY" \
  --elapsed-ms "$TOTAL_ELAPSED_MS" \
  --budget-ms "$FULL_BUDGET_MS" \
  --min-history "$FULL_MIN_HISTORY" \
  --extra-json "{\"command\":\"strict-golden + wasm-cross-host\",\"strict_elapsed_ms\":$STRICT_ELAPSED_MS,\"wasm_cross_host_elapsed_ms\":$WASM_ELAPSED_MS,\"strict_report\":\"$STRICT_REPORT\",\"wasm_report\":\"$WASM_REPORT\"}"

echo "full-cross-host-budget: ok total_elapsed_ms=$TOTAL_ELAPSED_MS budget_ms=$FULL_BUDGET_MS"
