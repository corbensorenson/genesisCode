#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if [[ "${GENESIS_GENERATED_AUTHORITY_STAGE:-0}" == "1" ]]; then
  export GENESIS_GC_REPAIR_UTILITY_REPEATS=1
fi

exec bash scripts/render_gc_repair_utility_report.sh \
  benchmarks/diagnostics/repair_utility/v0.1/report.json
