#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

PATH_TO_CHECK="."
MIN_FREE_KB="${GENESIS_MIN_FREE_KB:-1048576}" # 1 GiB default floor
CONTEXT="${GENESIS_DISK_CHECK_CONTEXT:-genesis}"
AUTO_RECLAIM="${GENESIS_DISK_AUTO_RECLAIM:-0}"
STRICT_MODE="${GENESIS_DISK_STRICT_MODE:-auto}"

usage() {
  cat <<'EOF'
Usage: scripts/check_disk_headroom.sh [options]

Options:
  --path <dir>      filesystem path to check (default: .)
  --min-kb <N>      minimum free KB required (default: GENESIS_MIN_FREE_KB or 1048576)
  --context <name>  label used in diagnostics (default: genesis)
  --auto-reclaim <0|1>  compatibility option; 1 is rejected because checks are read-only (default: 0)
  --strict <auto|0|1>   fail hard after retry (auto => CI=true only)
  -h, --help        show help
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --path)
      PATH_TO_CHECK="${2:-}"
      shift 2
      ;;
    --min-kb)
      MIN_FREE_KB="${2:-}"
      shift 2
      ;;
    --context)
      CONTEXT="${2:-}"
      shift 2
      ;;
    --auto-reclaim)
      AUTO_RECLAIM="${2:-}"
      shift 2
      ;;
    --strict)
      STRICT_MODE="${2:-}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "check-disk-headroom: unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

[[ "$MIN_FREE_KB" =~ ^[0-9]+$ ]] || {
  echo "check-disk-headroom: --min-kb must be numeric" >&2
  exit 2
}
[[ "$AUTO_RECLAIM" == "0" || "$AUTO_RECLAIM" == "1" ]] || {
  echo "check-disk-headroom: --auto-reclaim must be 0 or 1" >&2
  exit 2
}
if [[ "$AUTO_RECLAIM" == "1" ]]; then
  echo "check-disk-headroom: checks are read-only; dry-run and review scripts/reclaim_build_space.sh --profile dev-clean, then execute the confirmed plan and rerun with --auto-reclaim 0" >&2
  exit 2
fi
[[ "$STRICT_MODE" == "auto" || "$STRICT_MODE" == "0" || "$STRICT_MODE" == "1" ]] || {
  echo "check-disk-headroom: --strict must be auto, 0, or 1" >&2
  exit 2
}

if [[ ! -e "$PATH_TO_CHECK" ]]; then
  echo "check-disk-headroom: path does not exist: $PATH_TO_CHECK" >&2
  exit 2
fi

read_free_kb() {
  local free_kb
  free_kb="$(df -Pk "$PATH_TO_CHECK" | awk 'NR==2 {print $4}')"
  [[ "$free_kb" =~ ^[0-9]+$ ]] || {
    echo "check-disk-headroom: unable to read free space for $PATH_TO_CHECK" >&2
    return 2
  }
  echo "$free_kb"
}

if [[ "$STRICT_MODE" == "auto" ]]; then
  if [[ "${CI:-}" == "true" ]]; then
    STRICT_MODE="1"
  else
    STRICT_MODE="0"
  fi
fi

FREE_KB="$(read_free_kb)"
echo "${CONTEXT}: disk headroom precheck free_kb=${FREE_KB} required_kb=${MIN_FREE_KB}"

if (( FREE_KB < MIN_FREE_KB )); then
  echo "${CONTEXT}: insufficient disk headroom: ${FREE_KB}KB free, need at least ${MIN_FREE_KB}KB." >&2
  echo "${CONTEXT}: for cleanup, dry-run and review scripts/reclaim_build_space.sh --profile dev-clean, then execute the confirmed plan." >&2
  if [[ "$STRICT_MODE" == "1" ]]; then
    exit 2
  fi
  echo "${CONTEXT}: continuing in non-strict mode because CI!=true (set --strict 1 to fail locally)." >&2
  exit 0
fi

echo "${CONTEXT}: disk headroom ok (${FREE_KB}KB free)"
