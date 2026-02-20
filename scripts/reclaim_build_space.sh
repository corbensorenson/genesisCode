#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

MODE="safe"

usage() {
  cat <<'EOF'
Usage: scripts/reclaim_build_space.sh [--safe|--aggressive]

Modes:
  --safe        remove incremental build cache only (default)
  --aggressive  run `cargo clean` for full target cleanup
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --safe)
      MODE="safe"
      shift
      ;;
    --aggressive)
      MODE="aggressive"
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "reclaim-build-space: unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

TARGET_KB_BEFORE="$(du -sk target 2>/dev/null | awk '{print $1}' || echo 0)"

if [[ "$MODE" == "safe" ]]; then
  rm -rf target/debug/incremental target/tmp
else
  cargo clean
fi

TARGET_KB_AFTER="$(du -sk target 2>/dev/null | awk '{print $1}' || echo 0)"
RECLAIMED_KB=$(( TARGET_KB_BEFORE - TARGET_KB_AFTER ))
if (( RECLAIMED_KB < 0 )); then
  RECLAIMED_KB=0
fi

echo "reclaim-build-space: mode=${MODE} reclaimed_kb=${RECLAIMED_KB}"
