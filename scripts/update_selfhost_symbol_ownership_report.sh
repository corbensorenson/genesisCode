#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

PERSISTENT_REPORT_PATH="${GENESIS_SELFHOST_SYMBOL_OWNERSHIP_REPORT:-.genesis/perf/selfhost_symbol_ownership_report.json}"
PERSISTENT_HISTORY_PATH="${GENESIS_SELFHOST_SYMBOL_OWNERSHIP_HISTORY:-.genesis/perf/selfhost_symbol_ownership_history.jsonl}"

exec bash scripts/render_selfhost_symbol_ownership_report.sh \
  "$PERSISTENT_REPORT_PATH" \
  "$PERSISTENT_HISTORY_PATH" \
  "$PERSISTENT_HISTORY_PATH"
