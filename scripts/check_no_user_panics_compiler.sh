#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/genesis-panic-compiler.XXXXXX")"
trap 'rm -rf "$TMP_DIR"' EXIT

bash "$ROOT_DIR/scripts/render_no_user_panics_report.sh" \
  "$TMP_DIR/no_user_panics_report.json" \
  "$TMP_DIR/no_user_panics_history.jsonl"
