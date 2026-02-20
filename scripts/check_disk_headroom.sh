#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

PATH_TO_CHECK="."
MIN_FREE_KB="${GENESIS_MIN_FREE_KB:-1048576}" # 1 GiB default floor
CONTEXT="${GENESIS_DISK_CHECK_CONTEXT:-genesis}"

usage() {
  cat <<'EOF'
Usage: scripts/check_disk_headroom.sh [options]

Options:
  --path <dir>      filesystem path to check (default: .)
  --min-kb <N>      minimum free KB required (default: GENESIS_MIN_FREE_KB or 1048576)
  --context <name>  label used in diagnostics (default: genesis)
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

if [[ ! -e "$PATH_TO_CHECK" ]]; then
  echo "check-disk-headroom: path does not exist: $PATH_TO_CHECK" >&2
  exit 2
fi

FREE_KB="$(df -Pk "$PATH_TO_CHECK" | awk 'NR==2 {print $4}')"
[[ "$FREE_KB" =~ ^[0-9]+$ ]] || {
  echo "check-disk-headroom: unable to read free space for $PATH_TO_CHECK" >&2
  exit 2
}

if (( FREE_KB < MIN_FREE_KB )); then
  echo "${CONTEXT}: insufficient disk headroom: ${FREE_KB}KB free, need at least ${MIN_FREE_KB}KB." >&2
  echo "${CONTEXT}: run scripts/reclaim_build_space.sh --safe (or --aggressive) and retry." >&2
  exit 2
fi

echo "${CONTEXT}: disk headroom ok (${FREE_KB}KB free)"
