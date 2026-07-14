#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

exec bash scripts/render_kernel_tcb_contract_report.sh \
  ".genesis/perf/kernel_tcb_contract_report.json"
