#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/genesis-panic-policy.XXXXXX")"
trap 'rm -rf "$TMP_DIR"' EXIT

python3 "$ROOT_DIR/scripts/lib/panic_policy.py" \
  --root "$ROOT_DIR" \
  --out "$TMP_DIR/no_user_panics_static_report.json"
