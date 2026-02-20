#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

POLICY_FILE="${GENESIS_SOURCE_SIZE_POLICY:-policies/source_size_budget.toml}"

if [[ ! -f "$POLICY_FILE" ]]; then
  echo "gc-source-size-budget: missing policy file: $POLICY_FILE" >&2
  exit 1
fi

parse_positive_int() {
  local key="$1"
  local raw
  raw="$(awk -F= -v k="$key" '$1 ~ ("^" k "[[:space:]]*$") {gsub(/[[:space:]]/, "", $2); print $2; exit}' "$POLICY_FILE")"
  if [[ -z "$raw" ]]; then
    echo ""
    return 0
  fi
  if [[ ! "$raw" =~ ^[0-9]+$ || "$raw" -le 0 ]]; then
    echo "gc-source-size-budget: $key must be a positive integer in $POLICY_FILE" >&2
    exit 1
  fi
  echo "$raw"
}

parse_array_tokens() {
  local key="$1"
  local line
  line="$(awk -v k="$key" '$0 ~ ("^" k "[[:space:]]*=") {print; exit}' "$POLICY_FILE")"
  if [[ -z "$line" ]]; then
    return 0
  fi
  printf "%s\n" "$line" | grep -oE '"[^"]+"' | tr -d '"'
}

contains_token_match() {
  local needle="$1"
  shift
  local token
  for token in "$@"; do
    if [[ -n "$token" && "$needle" == *"$token"* ]]; then
      return 0
    fi
  done
  return 1
}

GC_MAX_LINES="$(parse_positive_int gc_max_lines)"
GC_TARGET_LINES="$(parse_positive_int gc_target_lines)"

if [[ -z "$GC_MAX_LINES" ]]; then
  echo "gc-source-size-budget: gc_max_lines is required in $POLICY_FILE" >&2
  exit 1
fi

EXCLUDES=()
while IFS= read -r token; do
  [[ -n "$token" ]] && EXCLUDES+=("$token")
done < <(parse_array_tokens exclude_substrings || true)

GC_EXCLUDE_PATHS=()
while IFS= read -r token; do
  [[ -n "$token" ]] && GC_EXCLUDE_PATHS+=("$token")
done < <(parse_array_tokens gc_exclude_paths || true)

GC_TARGET_EXCLUDE_PATHS=()
while IFS= read -r token; do
  [[ -n "$token" ]] && GC_TARGET_EXCLUDE_PATHS+=("$token")
done < <(parse_array_tokens gc_target_exclude_paths || true)

TMP_COUNTS="$(mktemp)"
TMP_TARGET_DEBT="$(mktemp)"
cleanup() {
  rm -f "$TMP_COUNTS" "$TMP_TARGET_DEBT"
}
trap cleanup EXIT

violations=0
while IFS= read -r abs; do
  [[ -n "$abs" ]] || continue
  rel="${abs#$ROOT_DIR/}"
  if contains_token_match "$rel" "${EXCLUDES[@]:-}"; then
    continue
  fi
  if contains_token_match "$rel" "${GC_EXCLUDE_PATHS[@]:-}"; then
    continue
  fi

  lines="$(wc -l < "$abs" | tr -d '[:space:]')"
  printf "%s %s\n" "$lines" "$rel" >> "$TMP_COUNTS"
  if (( lines > GC_MAX_LINES )); then
    echo "gc-source-size-budget: violation $rel has $lines lines (max $GC_MAX_LINES)" >&2
    violations=1
  fi
  if [[ -n "$GC_TARGET_LINES" && "$lines" -gt "$GC_TARGET_LINES" ]]; then
    if contains_token_match "$rel" "${GC_TARGET_EXCLUDE_PATHS[@]:-}"; then
      printf "%s %s\n" "$lines" "$rel" >> "$TMP_TARGET_DEBT"
    else
      echo "gc-source-size-budget: target violation $rel has $lines lines (target $GC_TARGET_LINES)" >&2
      violations=1
    fi
  fi
done < <(find "$ROOT_DIR/prelude/modules" "$ROOT_DIR/selfhost" -maxdepth 1 -type f -name '*.gc' | sort)

echo "gc-source-size-budget: policy=$POLICY_FILE gc_max_lines=$GC_MAX_LINES gc_target_lines=${GC_TARGET_LINES:-<disabled>}"
echo "gc-source-size-budget: top gc authoring sources by line count:"
if [[ -s "$TMP_COUNTS" ]]; then
  sort -nr "$TMP_COUNTS" | head -n 12 | awk '{printf "  %5s  %s\n", $1, $2}'
else
  echo "      0  <none>"
fi

if [[ -n "$GC_TARGET_LINES" ]]; then
  if [[ -s "$TMP_TARGET_DEBT" ]]; then
    echo "gc-source-size-budget: gc target debt allowlist (must trend to zero):"
    sort -nr "$TMP_TARGET_DEBT" | awk '{printf "  %5s  %s\n", $1, $2}'
  else
    echo "gc-source-size-budget: gc target debt allowlist is empty"
  fi
fi

if (( violations != 0 )); then
  exit 1
fi

echo "gc-source-size-budget: ok"
