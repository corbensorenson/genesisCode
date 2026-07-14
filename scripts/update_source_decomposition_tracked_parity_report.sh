#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

REPORT_PATH="${GENESIS_SOURCE_DECOMPOSITION_TRACKED_PARITY_REPORT:-.genesis/perf/source_decomposition_tracked_parity_report.json}"
POLICY_INPUT="${GENESIS_SOURCE_DECOMPOSITION_POLICY:-policies/source_decomposition_progress.toml}"

exec bash scripts/render_source_decomposition_tracked_parity_report.sh \
  "$REPORT_PATH" \
  "$POLICY_INPUT"
