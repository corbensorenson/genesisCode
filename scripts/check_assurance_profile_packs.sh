#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

bash "$ROOT_DIR/scripts/render_assurance_profile_packs_report.sh"   "$TMP_DIR/assurance_profile_packs_report.json"   "$TMP_DIR/assurance_profile_packs_history.jsonl"
