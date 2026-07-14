#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/genesis-doc-complexity.XXXXXX")"
trap 'rm -rf "$TMP_DIR"' EXIT

exec bash scripts/render_doc_complexity_report.sh "$TMP_DIR/doc_complexity_report.json"
