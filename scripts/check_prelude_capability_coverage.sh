#!/usr/bin/env bash
set -euo pipefail

export LC_ALL=C

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

PRELUDE_FILES=(
  "prelude/modules/10_gfx.gc"
  "prelude/modules/11_gpu_compute.gc"
  "prelude/modules/20_editor.gc"
)
RUNNER_FILE="crates/gc_effects/src/runner.rs"

for f in "${PRELUDE_FILES[@]}"; do
  if [[ ! -f "$f" ]]; then
    echo "prelude-capability-coverage: missing file: $f"
    exit 1
  fi
done
if [[ ! -f "$RUNNER_FILE" ]]; then
  echo "prelude-capability-coverage: missing runner file: $RUNNER_FILE"
  exit 1
fi

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

WRAPPER_OPS="$TMP_DIR/wrapper_ops.txt"
RUNNER_OPS="$TMP_DIR/runner_ops.txt"
MISSING="$TMP_DIR/missing.txt"

rg -o --no-filename --pcre2 '\(quote\s+(gfx/[[:alnum:]_.:/-]+::[[:alnum:]_.:/-]+|gpu/compute::[[:alnum:]_.:/-]+|editor/[[:alnum:]_.:/-]+::[[:alnum:]_.:/-]+)\)' \
  "${PRELUDE_FILES[@]}" \
  | sed -E 's/^\(quote[[:space:]]+//; s/\)$//' \
  | sort -u >"$WRAPPER_OPS"

if [[ ! -s "$WRAPPER_OPS" ]]; then
  echo "prelude-capability-coverage: no gfx/gpu-compute/editor capability wrapper ops found"
  exit 1
fi

rg -o --no-filename --pcre2 '"(gfx/[[:alnum:]_.:/-]+::[[:alnum:]_.:/-]+|gpu/compute::[[:alnum:]_.:/-]+|editor/[[:alnum:]_.:/-]+::[[:alnum:]_.:/-]+)"' \
  "$RUNNER_FILE" \
  | tr -d '"' \
  | sort -u >"$RUNNER_OPS"

if [[ ! -s "$RUNNER_OPS" ]]; then
  echo "prelude-capability-coverage: no gfx/gpu-compute/editor ops found in runner dispatch"
  exit 1
fi

comm -23 "$WRAPPER_OPS" "$RUNNER_OPS" >"$MISSING" || true
if [[ -s "$MISSING" ]]; then
  echo "prelude-capability-coverage: runner is missing prelude wrapper ops:"
  cat "$MISSING"
  exit 1
fi

echo "prelude-capability-coverage: ok"
