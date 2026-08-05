#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if [[ "$#" -lt 3 ]]; then
  echo "usage: $0 <output-dir> (--pair-index <1-5> | --aggregate --target-report <path>... --pair-output <dir>...)" >&2
  exit 2
fi

OUTPUT_DIR="$1"
shift
MODE=""
PAIR_INDEX=""
PAIR_OUTPUTS=()
TARGET_REPORTS=()
while [[ "$#" -gt 0 ]]; do
  case "$1" in
    --pair-index)
      [[ -z "$MODE" || "$MODE" == "pair" ]] || {
        echo "release-full-measurement: --pair-index conflicts with --aggregate" >&2
        exit 2
      }
      MODE="pair"
      PAIR_INDEX="${2:-}"
      shift 2
      ;;
    --aggregate)
      [[ -z "$MODE" || "$MODE" == "aggregate" ]] || {
        echo "release-full-measurement: --aggregate conflicts with --pair-index" >&2
        exit 2
      }
      MODE="aggregate"
      shift
      ;;
    --pair-output)
      PAIR_OUTPUTS+=("${2:-}")
      shift 2
      ;;
    --target-report)
      TARGET_REPORTS+=("${2:-}")
      shift 2
      ;;
    *)
      echo "release-full-measurement: unknown argument: $1" >&2
      exit 2
      ;;
  esac
done

[[ -n "$MODE" ]] || {
  echo "release-full-measurement: exactly one of --pair-index or --aggregate is required" >&2
  exit 2
}

if [[ "$MODE" == "pair" ]]; then
  [[ -n "$PAIR_INDEX" && "${#PAIR_OUTPUTS[@]}" -eq 0 && "${#TARGET_REPORTS[@]}" -eq 0 ]] || {
    echo "release-full-measurement: pair mode requires one index and no aggregate inputs" >&2
    exit 2
  }
  ARGS=(run-pair --root "$ROOT_DIR" --output "$OUTPUT_DIR")
  ARGS+=(--pair-index "$PAIR_INDEX")
else
  [[ -z "$PAIR_INDEX" && "${#PAIR_OUTPUTS[@]}" -ge 2 && "${#TARGET_REPORTS[@]}" -eq 4 ]] || {
    echo "release-full-measurement: aggregate mode requires four target reports and at least two pair outputs" >&2
    exit 2
  }
  ARGS=(aggregate --root "$ROOT_DIR" --output "$OUTPUT_DIR")
  for pair_output in "${PAIR_OUTPUTS[@]}"; do
    [[ -n "$pair_output" ]] || {
      echo "release-full-measurement: pair output must not be empty" >&2
      exit 2
    }
    ARGS+=(--pair-output "$pair_output")
  done
  for report in "${TARGET_REPORTS[@]}"; do
    [[ -n "$report" ]] || {
      echo "release-full-measurement: target report must not be empty" >&2
      exit 2
    }
    ARGS+=(--target-report "$report")
  done
fi

exec python3 scripts/lib/release_full_measurement.py "${ARGS[@]}"
