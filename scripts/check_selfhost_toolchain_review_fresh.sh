#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

SIDECAR_PATH="${GENESIS_SELFHOST_TOOLCHAIN_REVIEW_PATH:-selfhost/toolchain.review.md}"

[[ -f "$SIDECAR_PATH" ]] || {
  echo "selfhost-toolchain-review-fresh: missing committed sidecar at $SIDECAR_PATH" >&2
  echo "selfhost-toolchain-review-fresh: run scripts/update_selfhost_toolchain_review.sh" >&2
  exit 1
}

TMP_FILE="$(mktemp)"
trap 'rm -f "$TMP_FILE"' EXIT

bash scripts/update_selfhost_toolchain_review.sh "$TMP_FILE" >/dev/null

if ! diff -u "$SIDECAR_PATH" "$TMP_FILE" >/dev/null; then
  echo "selfhost-toolchain-review-fresh: committed sidecar is stale." >&2
  echo "  expected: $SIDECAR_PATH matches deterministic review sidecar output" >&2
  echo "  fix: bash scripts/update_selfhost_toolchain_review.sh" >&2
  exit 1
fi

echo "selfhost-toolchain-review-fresh: ok"
