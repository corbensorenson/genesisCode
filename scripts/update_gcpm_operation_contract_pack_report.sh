#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

PERSISTENT_REPORT_PATH="${GENESIS_GCPM_OPERATION_CONTRACT_PACK_REPORT:-.genesis/perf/gcpm_operation_contract_pack_report.json}"
exec bash scripts/render_gcpm_operation_contract_pack_report.sh "$PERSISTENT_REPORT_PATH"
