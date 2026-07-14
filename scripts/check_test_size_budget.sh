#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

POLICY_FILE="${GENESIS_TEST_SIZE_POLICY:-policies/test_size_budget.toml}"
if [[ ! -f "$POLICY_FILE" ]]; then
  echo "test-size-budget: missing policy file: $POLICY_FILE" >&2
  exit 1
fi

parse_positive_int() {
  local key="$1"
  local raw
  raw="$(awk -F= -v k="$key" '$1 ~ ("^" k "[[:space:]]*$") {gsub(/[[:space:]]/, "", $2); print $2; exit}' "$POLICY_FILE")"
  if [[ -z "$raw" || ! "$raw" =~ ^[0-9]+$ || "$raw" -le 0 ]]; then
    echo "test-size-budget: $key must be a positive integer in $POLICY_FILE" >&2
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

TEST_MAX_LINES="$(parse_positive_int test_max_lines)"
TEST_TARGET_LINES="$(parse_positive_int test_target_lines)"
if (( TEST_TARGET_LINES > TEST_MAX_LINES )); then
  echo "test-size-budget: test_target_lines ($TEST_TARGET_LINES) must be <= test_max_lines ($TEST_MAX_LINES)" >&2
  exit 1
fi

DEBT_ALLOWLIST=()
while IFS= read -r token; do
  [[ -n "$token" ]] && DEBT_ALLOWLIST+=("$token")
done < <(parse_array_tokens target_debt_allowlist || true)

EXCLUDES=()
while IFS= read -r token; do
  [[ -n "$token" ]] && EXCLUDES+=("$token")
done < <(parse_array_tokens exclude_substrings || true)

TMP_COUNTS="$(mktemp)"
TMP_DEBT_HITS="$(mktemp)"
cleanup() {
  rm -f "$TMP_COUNTS" "$TMP_DEBT_HITS"
}
trap cleanup EXIT

violations=0
while IFS= read -r f; do
  [[ -n "$f" ]] || continue
  rel="${f#$ROOT_DIR/}"
  if contains_token_match "$rel" "${EXCLUDES[@]:-}"; then
    continue
  fi
  lines="$(wc -l < "$f" | tr -d '[:space:]')"
  printf "%s %s\n" "$lines" "$rel" >> "$TMP_COUNTS"

  if (( lines > TEST_MAX_LINES )); then
    echo "test-size-budget: violation $rel has $lines lines (max $TEST_MAX_LINES)" >&2
    violations=1
    continue
  fi

  if (( lines > TEST_TARGET_LINES )); then
    if contains_token_match "$rel" "${DEBT_ALLOWLIST[@]:-}"; then
      printf "%s\n" "$rel" >> "$TMP_DEBT_HITS"
    else
      echo "test-size-budget: target violation $rel has $lines lines (target $TEST_TARGET_LINES)" >&2
      violations=1
    fi
  fi
done < <(find "$ROOT_DIR/crates" -type f -path '*/tests/*.rs' | sort)

if [[ "${#DEBT_ALLOWLIST[@]}" -gt 0 ]]; then
  for debt in "${DEBT_ALLOWLIST[@]}"; do
    if [[ ! -f "$ROOT_DIR/$debt" ]]; then
      echo "test-size-budget: debt allowlist entry does not exist: $debt" >&2
      violations=1
      continue
    fi
    if ! grep -Fxq "$debt" "$TMP_DEBT_HITS"; then
      lines="$(wc -l < "$ROOT_DIR/$debt" | tr -d '[:space:]')"
      if (( lines <= TEST_TARGET_LINES )); then
        echo "test-size-budget: stale debt allowlist entry (now <= target): $debt ($lines <= $TEST_TARGET_LINES)" >&2
        violations=1
      else
        # Still above target but not encountered due to exclude mismatch.
        echo "test-size-budget: debt allowlist entry not tracked by scanner (check excludes/path): $debt" >&2
        violations=1
      fi
    fi
  done
fi

echo "test-size-budget: policy=$POLICY_FILE test_max_lines=$TEST_MAX_LINES test_target_lines=$TEST_TARGET_LINES"
echo "test-size-budget: top test files by line count:"
if [[ -s "$TMP_COUNTS" ]]; then
  sort -nr "$TMP_COUNTS" | head -n 12 | awk '{printf "  %5s  %s\n", $1, $2}'
else
  echo "      0  <none>"
fi

if [[ "${#DEBT_ALLOWLIST[@]}" -gt 0 ]]; then
  echo "test-size-budget: debt allowlist entries (${#DEBT_ALLOWLIST[@]}):"
  for debt in "${DEBT_ALLOWLIST[@]}"; do
    lines="$(wc -l < "$ROOT_DIR/$debt" | tr -d '[:space:]')"
    printf "  %5s  %s\n" "$lines" "$debt"
  done
fi

if (( violations != 0 )); then
  exit 1
fi

echo "test-size-budget: ok"
