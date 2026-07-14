#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

PERSISTENT_REPORT_PATH="${GENESIS_VCS_SELFHOST_CONTRACT_REPORT:-.genesis/perf/vcs_selfhost_contract_report.json}"
exec bash scripts/render_vcs_selfhost_contract_report.sh "$PERSISTENT_REPORT_PATH"
