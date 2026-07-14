#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

PERSISTENT_REPORT_PATH="${GENESIS_FOUNDATION_STDLIB_CONFORMANCE_REPORT:-.genesis/perf/foundation_stdlib_conformance_report.json}"
PERSISTENT_HISTORY_PATH="${GENESIS_FOUNDATION_STDLIB_CONFORMANCE_HISTORY:-.genesis/perf/foundation_stdlib_conformance_history.jsonl}"
exec bash scripts/render_foundation_stdlib_conformance_report.sh \
  "$PERSISTENT_REPORT_PATH" "$PERSISTENT_HISTORY_PATH" "$PERSISTENT_HISTORY_PATH"
