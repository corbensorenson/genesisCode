#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

exec python3 scripts/lib/evidence_storage.py \
  render-fixture-catalog \
  --output docs/program/EVIDENCE_FIXTURE_CLASSIFICATION_v0.1.json \
  "$@"
