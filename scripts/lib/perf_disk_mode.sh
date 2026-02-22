#!/usr/bin/env bash
set -euo pipefail

# Resolve disk strictness mode used by perf-oriented gates.
# - auto: strict in CI, non-strict locally (delegated to check_disk_headroom.sh)
# - 1: always strict
# - 0: never strict
genesis_resolve_perf_disk_strict_mode() {
  local mode="${GENESIS_PERF_DISK_STRICT_MODE:-auto}"
  case "$mode" in
    auto|0|1)
      echo "$mode"
      ;;
    *)
      echo "perf-disk-mode: invalid GENESIS_PERF_DISK_STRICT_MODE='$mode' (expected auto|0|1)" >&2
      return 2
      ;;
  esac
}
