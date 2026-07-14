#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/perf_disk_mode.sh"

genesis_heavy_gate_preflight() {
  local root_dir="$1"
  local context="$2"
  local min_free_kb="$3"
  local tmp_root="$4"
  local auto_reclaim="${5:-0}"
  local strict_mode="${6:-}"

  if [[ -z "$strict_mode" ]]; then
    strict_mode="$(genesis_resolve_perf_disk_strict_mode)"
  fi

  [[ "$min_free_kb" =~ ^[0-9]+$ ]] || {
    echo "${context}: min_free_kb must be numeric (got '$min_free_kb')" >&2
    return 2
  }
  [[ "$auto_reclaim" == "0" || "$auto_reclaim" == "1" ]] || {
    echo "${context}: auto_reclaim must be 0 or 1 (got '$auto_reclaim')" >&2
    return 2
  }

  bash "$root_dir/scripts/check_disk_headroom.sh" \
    --path "$root_dir" \
    --context "$context" \
    --min-kb "$min_free_kb" \
    --auto-reclaim "$auto_reclaim" \
    --strict "$strict_mode"

  mkdir -p "$tmp_root"
  if [[ ! -d "$tmp_root" || ! -w "$tmp_root" ]]; then
    echo "${context}: tmp root is not writable: $tmp_root" >&2
    return 2
  fi

  local probe
  probe="$(mktemp -d "$tmp_root/${context}.XXXXXX")"
  rm -rf "$probe"

  export TMPDIR="$tmp_root"
  echo "${context}: tmp root ready at $TMPDIR"
}
