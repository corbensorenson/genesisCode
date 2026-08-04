#!/usr/bin/env bash

genesis_verify_health_profile_evidence() {
  local consumer_id="$1"
  local script_path="$2"
  shift 2
  local manifest="${GENESIS_HEALTH_EVIDENCE_MANIFEST:-}"
  local required="${GENESIS_HEALTH_EVIDENCE_REQUIRED:-0}"
  if [[ "$required" != "0" && "$required" != "1" ]]; then
    echo "health-profile-evidence: GENESIS_HEALTH_EVIDENCE_REQUIRED must be 0 or 1" >&2
    return 2
  fi
  if [[ -z "$manifest" ]]; then
    if [[ "$required" == "1" ]]; then
      echo "health-profile-evidence: required manifest is not configured for $consumer_id" >&2
      return 1
    fi
    return 0
  fi
  python3 "$ROOT_DIR/scripts/lib/health_profile_evidence.py" verify \
    --root "$ROOT_DIR" \
    --manifest "$manifest" \
    --consumer "$consumer_id" \
    --script "$script_path" \
    "$@"
}
