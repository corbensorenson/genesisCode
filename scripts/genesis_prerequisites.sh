#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if ! command -v python3 >/dev/null 2>&1; then
  echo "GenesisCode prerequisites: profile=unavailable platform=unknown ok=false" >&2
  echo "missing required bootstrap tool: python3 >=3.9.0 <4.0.0" >&2
  exit 2
fi

exec python3 scripts/lib/prerequisite_manifest.py diagnose "$@"
