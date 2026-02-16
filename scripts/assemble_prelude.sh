#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODULES_DIR="$ROOT_DIR/prelude/modules"
OUT_FILE="$ROOT_DIR/prelude/prelude.gc"

if [ ! -d "$MODULES_DIR" ]; then
  echo "modules directory missing: $MODULES_DIR" >&2
  exit 1
fi

modules="$(find "$MODULES_DIR" -maxdepth 1 -type f -name '*.gc' | sort)"
if [ -z "$modules" ]; then
  echo "no prelude modules found in $MODULES_DIR" >&2
  exit 1
fi

{
  while IFS= read -r f; do
    cat "$f"
    printf '\n'
  done <<EOF
$modules
EOF
} > "$OUT_FILE"
