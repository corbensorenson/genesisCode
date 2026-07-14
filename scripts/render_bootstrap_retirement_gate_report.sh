#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if [[ "$#" -ne 1 ]]; then
  echo "usage: $0 <report-output>" >&2
  exit 2
fi

REPORT_PATH="$1"

# Set to 0 to skip release-binary checks for faster local loops.
STRICT_RELEASE="${GENESIS_BOOTSTRAP_RETIREMENT_STRICT_RELEASE:-1}"
LOCAL_DEGRADED_MODE="${GENESIS_BOOTSTRAP_RETIREMENT_LOCAL_DEGRADED_MODE:-auto}"
DISK_MIN_FREE_KB="${GENESIS_RELEASE_GUARD_MIN_FREE_KB:-2097152}"
DISK_AUTO_RECLAIM="${GENESIS_BOOTSTRAP_RETIREMENT_DISK_AUTO_RECLAIM:-0}"

[[ "$STRICT_RELEASE" =~ ^[01]$ ]] || {
  echo "bootstrap-retirement-gate: GENESIS_BOOTSTRAP_RETIREMENT_STRICT_RELEASE must be 0 or 1" >&2
  exit 2
}
[[ "$LOCAL_DEGRADED_MODE" == "auto" || "$LOCAL_DEGRADED_MODE" == "0" || "$LOCAL_DEGRADED_MODE" == "1" ]] || {
  echo "bootstrap-retirement-gate: GENESIS_BOOTSTRAP_RETIREMENT_LOCAL_DEGRADED_MODE must be auto, 0, or 1" >&2
  exit 2
}
[[ "$DISK_AUTO_RECLAIM" == "0" || "$DISK_AUTO_RECLAIM" == "1" ]] || {
  echo "bootstrap-retirement-gate: GENESIS_BOOTSTRAP_RETIREMENT_DISK_AUTO_RECLAIM must be 0 or 1" >&2
  exit 2
}
[[ "$DISK_MIN_FREE_KB" =~ ^[0-9]+$ ]] || {
  echo "bootstrap-retirement-gate: GENESIS_RELEASE_GUARD_MIN_FREE_KB must be numeric" >&2
  exit 2
}

if [[ "$LOCAL_DEGRADED_MODE" == "auto" ]]; then
  if [[ "${CI:-}" == "true" ]]; then
    LOCAL_DEGRADED_MODE="0"
  else
    LOCAL_DEGRADED_MODE="1"
  fi
fi

write_report() {
  local status="$1"
  local reason="$2"
  local strict_executed="$3"
  python3 - "$REPORT_PATH" "$status" "$reason" "$STRICT_RELEASE" "$strict_executed" "$LOCAL_DEGRADED_MODE" <<'PY'
import json
import os
import pathlib
import sys
from datetime import datetime, timezone

report_path = pathlib.Path(sys.argv[1])
status = sys.argv[2]
reason = sys.argv[3]
strict_requested = sys.argv[4] == "1"
strict_executed = sys.argv[5] == "1"
local_degraded_enabled = sys.argv[6] == "1"

doc = {
    "kind": "genesis/bootstrap-retirement-gate-report-v0.1",
    "status": status,
    "reason": reason,
    "strict_release_requested": strict_requested,
    "strict_release_executed": strict_executed,
    "local_degraded_enabled": local_degraded_enabled,
    "ci": (os.environ.get("CI") == "true"),
    "timestamp_utc": datetime.now(timezone.utc).isoformat(timespec="seconds"),
}
report_path.parent.mkdir(parents=True, exist_ok=True)
report_path.write_text(json.dumps(doc, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"bootstrap-retirement-gate: wrote report {report_path}")
PY
}

bash scripts/check_old_bootstrap_retirement.sh
bash scripts/check_rust_engine_compat.sh
bash scripts/selfhost_default_profile_guard.sh

STRICT_RELEASE_EXECUTED="$STRICT_RELEASE"
STATUS="ok"
REASON="strict-release-passed"

if [[ "$STRICT_RELEASE" == "1" ]]; then
  if ! bash scripts/check_disk_headroom.sh \
    --path "$ROOT_DIR" \
    --context "bootstrap-retirement-gate" \
    --min-kb "$DISK_MIN_FREE_KB" \
    --auto-reclaim "$DISK_AUTO_RECLAIM" \
    --strict 1; then
    if [[ "$LOCAL_DEGRADED_MODE" == "1" ]]; then
      STRICT_RELEASE_EXECUTED="0"
      STATUS="degraded"
      REASON="insufficient-disk-headroom"
      echo "bootstrap-retirement-gate: degraded local mode activated (release guard skipped due constrained disk headroom)." >&2
    else
      write_report "fail" "insufficient-disk-headroom" "0"
      echo "bootstrap-retirement-gate: fail (strict release guard requires >=${DISK_MIN_FREE_KB}KB free; set GENESIS_BOOTSTRAP_RETIREMENT_LOCAL_DEGRADED_MODE=1 for local degraded mode)." >&2
      exit 2
    fi
  fi
fi

if [[ "$STRICT_RELEASE_EXECUTED" == "1" ]]; then
  bash scripts/selfhost_release_profile_guard.sh
fi

write_report "$STATUS" "$REASON" "$STRICT_RELEASE_EXECUTED"
if [[ "$STATUS" == "degraded" ]]; then
  echo "bootstrap-retirement-gate: degraded (strict_release_requested=$STRICT_RELEASE strict_release_executed=$STRICT_RELEASE_EXECUTED reason=$REASON)"
else
  echo "bootstrap-retirement-gate: ok (strict_release=$STRICT_RELEASE)"
fi
