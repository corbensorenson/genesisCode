#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

exec python3 scripts/lib/release_evidence_execution.py --root "$ROOT_DIR" "$@"
