#!/usr/bin/env bash
set -euo pipefail

genesis_profile_gate_now_ms() {
  python3 - <<'PY'
import time
print(int(time.time() * 1000))
PY
}

genesis_profile_gate_emit_runtime_report() {
  local profile="$1"
  local kind="$2"
  local report_path="$3"
  local history_path="$4"
  local start_ms="$5"
  local budget_ms="$6"
  local min_history="${7:-1}"
  local extra_json="${8:-}"

  local end_ms
  local elapsed_ms
  end_ms="$(genesis_profile_gate_now_ms)"
  elapsed_ms=$((end_ms - start_ms))

  python3 scripts/lib/profile_runtime_budget.py \
    --profile "$profile" \
    --kind "$kind" \
    --report "$report_path" \
    --history "$history_path" \
    --elapsed-ms "$elapsed_ms" \
    --budget-ms "$budget_ms" \
    --min-history "$min_history" \
    --extra-json "$extra_json"
}
