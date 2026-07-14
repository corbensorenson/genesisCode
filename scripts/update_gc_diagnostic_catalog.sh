#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT
python3 scripts/lib/gc_diagnostic_catalog.py --render >"$tmp"
mv "$tmp" docs/spec/GC_DIAGNOSTIC_CATALOG_v0.1.json
trap - EXIT
python3 scripts/lib/gc_diagnostic_catalog.py --check
