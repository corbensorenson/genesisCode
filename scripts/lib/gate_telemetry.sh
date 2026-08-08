#!/usr/bin/env bash

genesis_gate_telemetry_event() {
  local kind="$1"
  local count="${2:-1}"
  local event_file="${GENESIS_GATE_TELEMETRY_EVENT_FILE:-}"
  [[ -n "$event_file" ]] || return 0
  [[ "$kind" == "cache-hit" || "$kind" == "network-attempt" ]] || {
    echo "gate-telemetry: unsupported event kind: $kind" >&2
    return 2
  }
  [[ "$count" =~ ^[1-9][0-9]*$ ]] || {
    echo "gate-telemetry: event count must be positive" >&2
    return 2
  }
  printf '{"count":%s,"kind":"%s"}\n' "$count" "$kind" >>"$event_file"
}

genesis_gate_telemetry_reexec() {
  local script_path="$1"
  shift
  local scripts_dir root_dir entrypoint budget_enforce
  scripts_dir="$(cd "$(dirname "$script_path")" && pwd)"
  root_dir="$(cd "$scripts_dir/.." && pwd)"
  entrypoint="scripts/$(basename "$script_path")"
  if [[ "${GENESIS_GATE_TELEMETRY_ACTIVE_ENTRYPOINT:-}" == "$entrypoint" || \
        "${GENESIS_GATE_TELEMETRY_DISABLE:-0}" == "1" ]]; then
    return 0
  fi
  budget_enforce="${GENESIS_GATE_BUDGET_ENFORCE:-1}"
  [[ "$budget_enforce" == "0" || "$budget_enforce" == "1" ]] || {
    echo "gate-telemetry: GENESIS_GATE_BUDGET_ENFORCE must be 0 or 1" >&2
    return 2
  }
  GENESIS_GATE_BUDGET_ENFORCE="$budget_enforce" exec python3 "$root_dir/scripts/lib/gate_telemetry.py" \
    --root "$root_dir" \
    --entrypoint "$entrypoint" \
    --emit stderr \
    -- bash "$script_path" "$@"
}
