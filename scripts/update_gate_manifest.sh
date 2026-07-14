#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

tmp="$(mktemp "${TMPDIR:-/tmp}/genesis-gates.XXXXXX")"
trap 'rm -f "$tmp"' EXIT
python3 scripts/lib/gate_manifest.py --render > "$tmp"
mv "$tmp" genesis.gates.json
trap - EXIT
echo "update-gate-manifest: wrote genesis.gates.json"
