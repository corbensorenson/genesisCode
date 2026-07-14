#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/genesis-cargo-target-policy.XXXXXX")"
trap 'rm -rf "$TMP_DIR"' EXIT

exec bash scripts/render_cargo_target_dir_policy_report.sh \
  "$TMP_DIR/cargo_target_dir_policy_report.json" \
  "$TMP_DIR/cargo_target_dir_policy_history.jsonl"
