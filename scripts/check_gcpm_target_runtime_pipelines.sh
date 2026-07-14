#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

exec bash scripts/render_gcpm_target_runtime_pipelines_report.sh \
  "$TMP_DIR/gcpm_target_runtime_evidence_report.json" \
  "$TMP_DIR/gcpm_target_runtime_evidence"
