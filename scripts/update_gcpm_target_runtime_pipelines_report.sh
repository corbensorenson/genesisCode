#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

REPORT_PATH="${GENESIS_GCPM_TARGET_RUNTIME_EVIDENCE_REPORT:-.genesis/perf/gcpm_target_runtime_evidence_report.json}"
ARTIFACT_DIR="${GENESIS_GCPM_TARGET_RUNTIME_EVIDENCE_DIR:-.genesis/perf/gcpm_target_runtime_evidence}"

exec bash scripts/render_gcpm_target_runtime_pipelines_report.sh \
  "$REPORT_PATH" \
  "$ARTIFACT_DIR"
