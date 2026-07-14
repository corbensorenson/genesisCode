#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

exec bash scripts/render_selfhost_toolchain_review.sh "${1:-selfhost/toolchain.review.md}"
