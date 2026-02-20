#!/usr/bin/env bash
set -euo pipefail

export LC_ALL=C

genesis_now_ns() {
  python3 - <<'PY'
import time
print(time.time_ns())
PY
}

genesis_measure_ms_once() {
  local label="$1"
  shift
  local start_ns end_ns elapsed_ms

  if [[ -n "${GENESIS_MEASURE_FAIL_LABEL:-}" && "$label" == "${GENESIS_MEASURE_FAIL_LABEL}" ]]; then
    echo "measure: forced failure for label '${label}'" >&2
    return 1
  fi

  start_ns="$(genesis_now_ns)"
  if ! "$@" >/dev/null; then
    echo "measure: command failed for ${label}: $*" >&2
    return 1
  fi
  end_ns="$(genesis_now_ns)"
  elapsed_ms="$(( (end_ns - start_ns) / 1000000 ))"

  MEASURE_LAST_LABEL="$label"
  MEASURE_LAST_MS="$elapsed_ms"
}

genesis_measure_best_of_ms() {
  local label="$1"
  local warmups="$2"
  local repeats="$3"
  shift 3

  local i best_ms

  for ((i = 0; i < warmups; i++)); do
    if ! "$@" >/dev/null; then
      echo "measure: warmup command failed for ${label}: $*" >&2
      return 1
    fi
  done

  best_ms=""
  for ((i = 0; i < repeats; i++)); do
    genesis_measure_ms_once "$label" "$@"
    if [[ -z "$best_ms" || "$MEASURE_LAST_MS" -lt "$best_ms" ]]; then
      best_ms="$MEASURE_LAST_MS"
    fi
  done

  MEASURE_LAST_LABEL="$label"
  MEASURE_LAST_MS="$best_ms"
}
