#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
exec python3 "$ROOT_DIR/scripts/lib/prebuilt_evidence_verify.py" --root "$ROOT_DIR" "$@"
