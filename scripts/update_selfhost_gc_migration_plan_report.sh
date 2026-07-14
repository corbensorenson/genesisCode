#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPORT_OUT="${GENESIS_SELFHOST_GC_MIGRATION_PLAN_REPORT:-$ROOT_DIR/.genesis/perf/selfhost_gc_migration_plan_report.json}"

exec bash "$ROOT_DIR/scripts/render_selfhost_gc_migration_plan_report.sh" "$REPORT_OUT"
