#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

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

parse_gc_source_roots() {
  local saw=0
  local token
  while IFS= read -r token; do
    [[ -n "$token" ]] || continue
    saw=1
    printf "%s\n" "$token"
  done < <(parse_array_tokens gc_source_roots || true)

  if (( saw == 0 )); then
    printf "%s\n" "prelude/modules"
    printf "%s\n" "selfhost"
    printf "%s\n" "prelude/prelude.gc"
  fi
}

collect_gc_sources() {
  local root_spec="$1"
  local abs_root
  if [[ "$root_spec" == /* ]]; then
    abs_root="$root_spec"
  else
    abs_root="$ROOT_DIR/$root_spec"
  fi

  if [[ -f "$abs_root" ]]; then
    if [[ "$abs_root" == *.gc ]]; then
      printf "%s\n" "$abs_root"
    fi
    return 0
  fi
  if [[ -d "$abs_root" ]]; then
    find "$abs_root" -type f -name '*.gc'
    return 0
  fi

  echo "source-size-budget: configured gc_source_roots entry does not exist: $root_spec" >&2
  exit 1
}

RUST_MAX_LINES="$(parse_positive_int rust_max_lines 1)"
GC_MAX_LINES="$(parse_positive_int gc_max_lines 0)"
RUST_TARGET_LINES="$(parse_positive_int rust_target_lines 0)"
GC_TARGET_LINES="$(parse_positive_int gc_target_lines 0)"

EXCLUDES=()
while IFS= read -r token; do
  [[ -n "$token" ]] && EXCLUDES+=("$token")
done < <(parse_array_tokens exclude_substrings || true)

GC_GENERATED_EXCLUDE_PATHS=()
while IFS= read -r token; do
  [[ -n "$token" ]] && GC_GENERATED_EXCLUDE_PATHS+=("$token")
done < <(parse_array_tokens gc_generated_exclude_paths || true)

RUST_TARGET_EXCLUDE_PATHS=()
while IFS= read -r token; do
  [[ -n "$token" ]] && RUST_TARGET_EXCLUDE_PATHS+=("$token")
done < <(parse_array_tokens rust_target_exclude_paths || true)

GC_TARGET_EXCLUDE_PATHS=()
while IFS= read -r token; do
  [[ -n "$token" ]] && GC_TARGET_EXCLUDE_PATHS+=("$token")
done < <(parse_array_tokens gc_target_exclude_paths || true)

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
TMP_TARGET_DEBT_RUST="$(mktemp)"
TMP_TARGET_DEBT_GC="$(mktemp)"
TMP_GC_DISCOVERED="$(mktemp)"
cleanup() {
  rm -f "$TMP_COUNTS"
  rm -f "$TMP_GC_COUNTS"
  rm -f "$TMP_TARGET_DEBT_RUST"
  rm -f "$TMP_TARGET_DEBT_GC"
  rm -f "$TMP_GC_DISCOVERED"
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
  if [[ -n "$RUST_TARGET_LINES" && "$lines" -gt "$RUST_TARGET_LINES" ]]; then
    if contains_token_match "$f" "${RUST_TARGET_EXCLUDE_PATHS[@]:-}"; then
      printf "%s %s\n" "$lines" "$f" >> "$TMP_TARGET_DEBT_RUST"
    else
      echo "source-size-budget: target violation $f has $lines lines (target $RUST_TARGET_LINES)" >&2
      violations=1
    fi
  fi
done < <(list_rust_source_files)

if [[ -n "$GC_MAX_LINES" ]]; then
  while IFS= read -r root_spec; do
    [[ -n "$root_spec" ]] || continue
    collect_gc_sources "$root_spec" >> "$TMP_GC_DISCOVERED"
  done < <(parse_gc_source_roots)
  sort -u "$TMP_GC_DISCOVERED" -o "$TMP_GC_DISCOVERED"

  while IFS= read -r f; do
    [[ -n "$f" ]] || continue
    rel="${f#$ROOT_DIR/}"
    if contains_token_match "$rel" "${EXCLUDES[@]:-}"; then
      continue
    fi
    if contains_token_match "$rel" "${GC_GENERATED_EXCLUDE_PATHS[@]:-}"; then
      continue
    fi

    lines="$(wc -l < "$f" | tr -d '[:space:]')"
    printf "%s %s\n" "$lines" "$rel" >> "$TMP_GC_COUNTS"
    if (( lines > GC_MAX_LINES )); then
      echo "source-size-budget: violation $rel has $lines lines (max $GC_MAX_LINES)" >&2
      violations=1
    fi
    if [[ -n "$GC_TARGET_LINES" && "$lines" -gt "$GC_TARGET_LINES" ]]; then
      if contains_token_match "$rel" "${GC_TARGET_EXCLUDE_PATHS[@]:-}"; then
        printf "%s %s\n" "$lines" "$rel" >> "$TMP_TARGET_DEBT_GC"
      else
        echo "source-size-budget: target violation $rel has $lines lines (target $GC_TARGET_LINES)" >&2
        violations=1
      fi
    fi
  done < "$TMP_GC_DISCOVERED"
fi

echo "source-size-budget: policy=$POLICY_FILE rust_max_lines=$RUST_MAX_LINES gc_max_lines=${GC_MAX_LINES:-<disabled>} rust_target_lines=${RUST_TARGET_LINES:-<disabled>} gc_target_lines=${GC_TARGET_LINES:-<disabled>}"
echo "source-size-budget: top production files by line count:"
sort -nr "$TMP_COUNTS" | awk 'NR <= 8 {printf "  %5s  %s\n", $1, $2}'
if [[ -n "$GC_MAX_LINES" ]]; then
  echo "source-size-budget: top gc authoring sources by line count:"
  if [[ -s "$TMP_GC_COUNTS" ]]; then
    sort -nr "$TMP_GC_COUNTS" | awk 'NR <= 8 {printf "  %5s  %s\n", $1, $2}'
  else
    echo "      0  <none>"
  fi
fi

if [[ -n "$RUST_TARGET_LINES" ]]; then
  if [[ -s "$TMP_TARGET_DEBT_RUST" ]]; then
    echo "source-size-budget: rust target debt allowlist (must trend to zero):"
    sort -nr "$TMP_TARGET_DEBT_RUST" | awk '{printf "  %5s  %s\n", $1, $2}'
  else
    echo "source-size-budget: rust target debt allowlist is empty"
  fi
fi

if [[ -n "$GC_TARGET_LINES" ]]; then
  if [[ -s "$TMP_TARGET_DEBT_GC" ]]; then
    echo "source-size-budget: gc target debt allowlist (must trend to zero):"
    sort -nr "$TMP_TARGET_DEBT_GC" | awk '{printf "  %5s  %s\n", $1, $2}'
  else
    echo "source-size-budget: gc target debt allowlist is empty"
  fi
fi

if (( violations != 0 )); then
  exit 1
fi

echo "source-size-budget: ok"
