#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPORT_PATH="${GENESIS_TEST_CHANGED_REPORT:-$ROOT_DIR/.genesis/perf/test_changed_fast_metrics.json}"
HISTORY_PATH="${GENESIS_TEST_CHANGED_HISTORY:-$ROOT_DIR/.genesis/perf/test_changed_fast_history.jsonl}"

exec bash "$ROOT_DIR/scripts/test_changed_fast.sh" "$@" \
  --report "$REPORT_PATH" \
  --history "$HISTORY_PATH"
