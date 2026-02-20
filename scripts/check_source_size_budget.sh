#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

POLICY_FILE="${GENESIS_SOURCE_SIZE_POLICY:-policies/source_size_budget.toml}"

if [[ ! -f "$POLICY_FILE" ]]; then
  echo "source-size-budget: missing policy file: $POLICY_FILE" >&2
  exit 1
fi

parse_positive_int() {
  local key="$1"
  local required="${2:-0}"
  local raw
  raw="$(awk -F= -v k="$key" '$1 ~ ("^" k "[[:space:]]*$") {gsub(/[[:space:]]/, "", $2); print $2; exit}' "$POLICY_FILE")"
  if [[ -z "$raw" ]]; then
    if [[ "$required" == "1" ]]; then
      echo "source-size-budget: missing required key '$key' in $POLICY_FILE" >&2
      exit 1
    fi
    echo ""
    return 0
  fi
  if [[ ! "$raw" =~ ^[0-9]+$ || "$raw" -le 0 ]]; then
    echo "source-size-budget: $key must be a positive integer in $POLICY_FILE" >&2
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

RUST_MAX_LINES="$(parse_positive_int rust_max_lines 1)"
GC_MAX_LINES="$(parse_positive_int gc_max_lines 0)"

EXCLUDES=()
while IFS= read -r token; do
  [[ -n "$token" ]] && EXCLUDES+=("$token")
done < <(parse_array_tokens exclude_substrings || true)

GC_EXCLUDE_PATHS=()
while IFS= read -r token; do
  [[ -n "$token" ]] && GC_EXCLUDE_PATHS+=("$token")
done < <(parse_array_tokens gc_exclude_paths || true)

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

list_rust_source_files() {
  if command -v rg >/dev/null 2>&1; then
    rg --files crates --glob '*.rs' | sort
  else
    find crates -type f -name '*.rs' | sort
  fi
}

TMP_COUNTS="$(mktemp)"
TMP_GC_COUNTS="$(mktemp)"
cleanup() {
  rm -f "$TMP_COUNTS"
  rm -f "$TMP_GC_COUNTS"
}
trap cleanup EXIT

violations=0
while IFS= read -r f; do
  [[ -n "$f" ]] || continue
  if contains_token_match "$f" "${EXCLUDES[@]:-}"; then
    continue
  fi

  lines="$(wc -l < "$f" | tr -d '[:space:]')"
  printf "%s %s\n" "$lines" "$f" >> "$TMP_COUNTS"
  if (( lines > RUST_MAX_LINES )); then
    echo "source-size-budget: violation $f has $lines lines (max $RUST_MAX_LINES)" >&2
    violations=1
  fi
done < <(list_rust_source_files)

if [[ -n "$GC_MAX_LINES" ]]; then
  while IFS= read -r f; do
    [[ -n "$f" ]] || continue
    rel="${f#$ROOT_DIR/}"
    if contains_token_match "$rel" "${EXCLUDES[@]:-}"; then
      continue
    fi
    if contains_token_match "$rel" "${GC_EXCLUDE_PATHS[@]:-}"; then
      continue
    fi

    lines="$(wc -l < "$f" | tr -d '[:space:]')"
    printf "%s %s\n" "$lines" "$rel" >> "$TMP_GC_COUNTS"
    if (( lines > GC_MAX_LINES )); then
      echo "source-size-budget: violation $rel has $lines lines (max $GC_MAX_LINES)" >&2
      violations=1
    fi
  done < <(find "$ROOT_DIR/prelude/modules" "$ROOT_DIR/selfhost" -maxdepth 1 -type f -name '*.gc' | sort)
fi

echo "source-size-budget: policy=$POLICY_FILE rust_max_lines=$RUST_MAX_LINES gc_max_lines=${GC_MAX_LINES:-<disabled>}"
echo "source-size-budget: top production files by line count:"
sort -nr "$TMP_COUNTS" | head -n 8 | awk '{printf "  %5s  %s\n", $1, $2}'
if [[ -n "$GC_MAX_LINES" ]]; then
  echo "source-size-budget: top gc authoring sources by line count:"
  if [[ -s "$TMP_GC_COUNTS" ]]; then
    sort -nr "$TMP_GC_COUNTS" | head -n 8 | awk '{printf "  %5s  %s\n", $1, $2}'
  else
    echo "      0  <none>"
  fi
fi

if (( violations != 0 )); then
  exit 1
fi

echo "source-size-budget: ok"
