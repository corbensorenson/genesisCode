#!/usr/bin/env bash
set -euo pipefail

# Default fast-loop alias for local iteration.
# Uses changed-aware execution first; delegate to --full for the old broad suite.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

usage() {
  cat <<'EOF'
Usage: scripts/test_fast.sh [--full] [args...]

Default:
  Runs scripts/test_changed_fast.sh (changed-aware fast loop).

--full:
  Runs scripts/test_fast_full.sh (broad legacy fast suite).
EOF
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

if [[ "${1:-}" == "--full" ]]; then
  shift
  exec bash scripts/test_fast_full.sh "$@"
fi

exec bash scripts/test_changed_fast.sh "$@"
