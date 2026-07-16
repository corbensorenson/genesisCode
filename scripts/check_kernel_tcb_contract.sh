#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/genesis-kernel-tcb.XXXXXX")"
trap 'rm -rf "$TMP_DIR"' EXIT

bash scripts/render_kernel_tcb_contract_report.sh \
  "$TMP_DIR/kernel_tcb_contract_report.json"
