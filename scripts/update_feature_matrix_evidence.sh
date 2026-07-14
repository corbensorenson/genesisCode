#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

# Compatibility entrypoint. The canonical updater renders every ledger-backed view.
exec bash scripts/update_capability_status_views.sh "$@"
