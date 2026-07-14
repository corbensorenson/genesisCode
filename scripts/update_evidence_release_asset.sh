#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if [[ "$#" -lt 2 || "$#" -gt 7 ]]; then
  echo "usage: scripts/update_evidence_release_asset.sh <E3|E4> <release-id> [bundle] [tree] [artifact-root] [trust-policy] [trust-policy-sha256]" >&2
  exit 64
fi

STORAGE_CLASS="$1"
RELEASE_ID="$2"
shift 2
OUTPUT_DIR=".genesis/release-assets/evidence/$STORAGE_CLASS/$RELEASE_ID"

exec bash scripts/render_evidence_release_asset.sh \
  "$OUTPUT_DIR" \
  "$STORAGE_CLASS" \
  "$RELEASE_ID" \
  "$@"
