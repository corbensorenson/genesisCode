#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

PERSISTENT_REPORT_PATH="${GENESIS_FUZZ_DIFFERENTIAL_HARDENING_REPORT:-.genesis/perf/fuzz_differential_hardening_report.json}"
PERSISTENT_HISTORY_PATH="${GENESIS_FUZZ_DIFFERENTIAL_HARDENING_HISTORY:-.genesis/perf/fuzz_differential_hardening_history.jsonl}"
exec bash scripts/render_fuzz_differential_hardening_report.sh \
  "$PERSISTENT_REPORT_PATH" "$PERSISTENT_HISTORY_PATH" "$PERSISTENT_HISTORY_PATH"
