#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

PERSISTENT_REPORT_PATH="${GENESIS_BOOTSTRAP_RETIREMENT_REPORT:-.genesis/perf/bootstrap_retirement_gate_report.json}"
exec bash scripts/render_bootstrap_retirement_gate_report.sh "$PERSISTENT_REPORT_PATH"
