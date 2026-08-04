#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if [[ "$#" -lt 5 ]]; then
  echo "usage: $0 <output-dir> <android-report> <edge-report> <ios-report> <service-runtime-report> [--pairs <2-5>]" >&2
  exit 2
fi

OUTPUT_DIR="$1"
shift
TARGET_REPORTS=("$1" "$2" "$3" "$4")
shift 4
PAIRS=2
while [[ "$#" -gt 0 ]]; do
  case "$1" in
    --pairs)
      PAIRS="${2:-}"
      shift 2
      ;;
    *)
      echo "release-full-measurement: unknown argument: $1" >&2
      exit 2
      ;;
  esac
done

ARGS=(
  run
  --root "$ROOT_DIR"
  --output "$OUTPUT_DIR"
  --pairs "$PAIRS"
)
for report in "${TARGET_REPORTS[@]}"; do
  ARGS+=(--target-report "$report")
done

exec python3 scripts/lib/release_full_measurement.py "${ARGS[@]}"
