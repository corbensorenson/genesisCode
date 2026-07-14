#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

exec bash scripts/render_gc_repair_utility_report.sh \
  benchmarks/diagnostics/repair_utility/v0.1/report.json
