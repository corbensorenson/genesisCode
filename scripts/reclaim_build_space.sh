#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "reclaim-build-space" \
  ".genesis/build/cargo" \
  "GENESIS_RECLAIM_BUILD_SPACE_CARGO_TARGET_DIR"

MODE="safe"
BUILD_ROOT="${GENESIS_RECLAIM_BUILD_ROOT:-$ROOT_DIR/.genesis/build}"
MAX_BUILD_KB="${GENESIS_RECLAIM_MAX_BUILD_KB:-33554432}" # 32 GiB
MAX_AGE_DAYS="${GENESIS_RECLAIM_MAX_AGE_DAYS:-7}"
declare -a PRESERVE_DIRS=()
PRESERVE_DIRS+=("$CARGO_TARGET_DIR")

usage() {
  cat <<'EOF'
Usage: scripts/reclaim_build_space.sh [--safe|--aggressive] [--max-build-kb <N>] [--max-age-days <N>] [--build-root <path>] [--preserve-dir <path>]

Modes:
  --safe        remove incremental/tmp caches and prune stale/old build roots (default)
  --aggressive  include `cargo clean` and prune build roots to zero budget (except preserved dirs)

Options:
  --max-build-kb <N>  maximum allowed KB for build root after reclamation (default: 8388608)
  --max-age-days <N>  remove build-root children older than N days before budget pruning (default: 7)
  --build-root <path> build root to prune (default: .genesis/build)
  --preserve-dir <path> keep this directory (and descendants) from deletion; can be repeated
EOF
}

normalize_path() {
  local path="$1"
  if [[ "$path" = /* ]]; then
    printf '%s\n' "$path"
  else
    printf '%s\n' "$ROOT_DIR/$path"
  fi
}

is_positive_integer() {
  local value="$1"
  [[ "$value" =~ ^[0-9]+$ ]]
}

should_preserve_path() {
  local candidate="$1"
  local preserved
  for preserved in "${PRESERVE_DIRS[@]}"; do
    [[ -z "$preserved" ]] && continue
    if [[ "$candidate" == "$preserved" || "$candidate" == "$preserved/"* ]]; then
      return 0
    fi
  done
  return 1
}

remove_dir_best_effort() {
  local path="$1"
  if rm -rf "$path" 2>/dev/null; then
    return 0
  fi
  echo "reclaim-build-space: warning: unable to remove $path (skipping)" >&2
  return 1
}

build_root_kb() {
  if [[ ! -d "$BUILD_ROOT" ]]; then
    echo "0"
    return 0
  fi
  du -sk "$BUILD_ROOT" 2>/dev/null | awk '{print $1}'
}

prune_incremental_dirs() {
  local root="$1"
  local removed_count=0
  [[ -d "$root" ]] || return 0

  local dir
  while IFS= read -r -d '' dir; do
    if should_preserve_path "$dir"; then
      continue
    fi
    if remove_dir_best_effort "$dir"; then
      removed_count=$((removed_count + 1))
    fi
  done < <(find "$root" -type d \( -name incremental -o -name tmp \) -print0 2>/dev/null)

  echo "$removed_count"
}

prune_build_root_by_age() {
  local cutoff_epoch="$1"
  local removed_count=0
  [[ -d "$BUILD_ROOT" ]] || {
    echo "0"
    return 0
  }

  local child
  for child in "$BUILD_ROOT"/*; do
    [[ -d "$child" ]] || continue
    if should_preserve_path "$child"; then
      continue
    fi
    local mtime
    mtime="$(stat -f '%m' "$child" 2>/dev/null || echo 0)"
    if [[ ! "$mtime" =~ ^[0-9]+$ ]]; then
      continue
    fi
    if (( mtime < cutoff_epoch )); then
      if remove_dir_best_effort "$child"; then
        removed_count=$((removed_count + 1))
      fi
    fi
  done

  echo "$removed_count"
}

prune_build_root_to_budget() {
  local max_kb="$1"
  local removed_count=0
  [[ -d "$BUILD_ROOT" ]] || {
    echo "0"
    return 0
  }

  local current_kb
  current_kb="$(build_root_kb)"
  if (( current_kb <= max_kb )); then
    echo "0"
    return 0
  fi

  local tmp
  tmp="$(mktemp)"

  local child
  for child in "$BUILD_ROOT"/*; do
    [[ -d "$child" ]] || continue
    if should_preserve_path "$child"; then
      continue
    fi
    local mtime
    local size_kb
    mtime="$(stat -f '%m' "$child" 2>/dev/null || echo 0)"
    size_kb="$(du -sk "$child" 2>/dev/null | awk '{print $1}')"
    [[ "$mtime" =~ ^[0-9]+$ ]] || mtime=0
    [[ "$size_kb" =~ ^[0-9]+$ ]] || size_kb=0
    printf '%s\t%s\t%s\n' "$mtime" "$size_kb" "$child" >> "$tmp"
  done

  if [[ ! -s "$tmp" ]]; then
    rm -f "$tmp"
    echo "0"
    return 0
  fi

  local sorted
  sorted="$(mktemp)"
  sort -n -k1,1 -k3,3 "$tmp" > "$sorted"
  rm -f "$tmp"

  local mtime
  local size_kb
  local path
  while IFS=$'\t' read -r mtime size_kb path; do
    if (( current_kb <= max_kb )); then
      break
    fi
    if ! remove_dir_best_effort "$path"; then
      continue
    fi
    removed_count=$((removed_count + 1))
    if (( size_kb > current_kb )); then
      current_kb=0
    else
      current_kb=$((current_kb - size_kb))
    fi
  done < "$sorted"
  rm -f "$sorted"

  echo "$removed_count"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --safe)
      MODE="safe"
      shift
      ;;
    --aggressive)
      MODE="aggressive"
      shift
      ;;
    --max-build-kb)
      MAX_BUILD_KB="${2:-}"
      shift 2
      ;;
    --max-age-days)
      MAX_AGE_DAYS="${2:-}"
      shift 2
      ;;
    --build-root)
      BUILD_ROOT="$(normalize_path "${2:-}")"
      shift 2
      ;;
    --preserve-dir)
      PRESERVE_DIRS+=("$(normalize_path "${2:-}")")
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "reclaim-build-space: unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if ! is_positive_integer "$MAX_BUILD_KB"; then
  echo "reclaim-build-space: --max-build-kb must be numeric" >&2
  exit 2
fi
if ! is_positive_integer "$MAX_AGE_DAYS"; then
  echo "reclaim-build-space: --max-age-days must be numeric" >&2
  exit 2
fi

FREE_KB_BEFORE="$(df -Pk "$ROOT_DIR" | awk 'NR==2 {print $4}')"
BUILD_KB_BEFORE="$(build_root_kb)"
INCREMENTAL_REMOVED=0
STALE_REMOVED=0
BUDGET_REMOVED=0

INCREMENTAL_REMOVED=$(( \
  $(prune_incremental_dirs "$CARGO_TARGET_DIR") + \
  $(prune_incremental_dirs "$BUILD_ROOT") \
))

CUTOFF_EPOCH=$(( $(date +%s) - (MAX_AGE_DAYS * 86400) ))
STALE_REMOVED="$(prune_build_root_by_age "$CUTOFF_EPOCH")"

if [[ "$MODE" == "safe" ]]; then
  BUDGET_REMOVED="$(prune_build_root_to_budget "$MAX_BUILD_KB")"
else
  cargo clean
  BUDGET_REMOVED="$(prune_build_root_to_budget 0)"
fi

BUILD_KB_AFTER="$(build_root_kb)"
FREE_KB_AFTER="$(df -Pk "$ROOT_DIR" | awk 'NR==2 {print $4}')"
RECLAIMED_BUILD_KB=$(( BUILD_KB_BEFORE - BUILD_KB_AFTER ))
RECLAIMED_FREE_KB=$(( FREE_KB_AFTER - FREE_KB_BEFORE ))
(( RECLAIMED_BUILD_KB < 0 )) && RECLAIMED_BUILD_KB=0
(( RECLAIMED_FREE_KB < 0 )) && RECLAIMED_FREE_KB=0

echo "reclaim-build-space: mode=${MODE} build_root=${BUILD_ROOT} max_build_kb=${MAX_BUILD_KB} max_age_days=${MAX_AGE_DAYS} incremental_removed=${INCREMENTAL_REMOVED} stale_removed=${STALE_REMOVED} budget_removed=${BUDGET_REMOVED} reclaimed_build_kb=${RECLAIMED_BUILD_KB} reclaimed_free_kb=${RECLAIMED_FREE_KB} build_kb_before=${BUILD_KB_BEFORE} build_kb_after=${BUILD_KB_AFTER} free_kb_before=${FREE_KB_BEFORE} free_kb_after=${FREE_KB_AFTER}"
