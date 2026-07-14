#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

POLICY_INPUT="${GENESIS_SOURCE_DECOMPOSITION_POLICY:-policies/source_decomposition_progress.toml}"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

exec bash scripts/render_source_decomposition_tracked_parity_report.sh \
  "$TMP_DIR/source_decomposition_tracked_parity_report.json" \
  "$POLICY_INPUT"
