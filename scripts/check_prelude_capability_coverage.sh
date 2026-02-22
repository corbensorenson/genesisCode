#!/usr/bin/env bash
set -euo pipefail

export LC_ALL=C

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

PRELUDE_FILES=(
  "prelude/modules/10_browser_host.gc"
  "prelude/modules/10_gfx.gc"
  "prelude/modules/11_gpu_compute.gc"
  "prelude/modules/20_editor.gc"
)
RUNNER_FILES=(
  "crates/gc_effects/src/runner_capability_dispatch.rs"
  "crates/gc_effects/src/runner_browser_host.rs"
)

for f in "${PRELUDE_FILES[@]}"; do
  if [[ ! -f "$f" ]]; then
    echo "prelude-capability-coverage: missing file: $f"
    exit 1
  fi
done
for f in "${RUNNER_FILES[@]}"; do
  if [[ ! -f "$f" ]]; then
    echo "prelude-capability-coverage: missing runner file: $f"
    exit 1
  fi
done

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

WRAPPER_OPS="$TMP_DIR/wrapper_ops.txt"
RUNNER_OPS="$TMP_DIR/runner_ops.txt"
MISSING="$TMP_DIR/missing.txt"

extract_wrapper_ops() {
  if command -v rg >/dev/null 2>&1; then
    rg -o --no-filename --pcre2 '\(quote\s+(browser/[[:alnum:]_.:/-]+::[[:alnum:]_.:/-]+|gfx/[[:alnum:]_.:/-]+::[[:alnum:]_.:/-]+|gpu/compute::[[:alnum:]_.:/-]+|editor/[[:alnum:]_.:/-]+::[[:alnum:]_.:/-]+)\)' "${PRELUDE_FILES[@]}"
  else
    grep -Eho '\(quote[[:space:]]+(browser/[[:alnum:]_.:/-]+::[[:alnum:]_.:/-]+|gfx/[[:alnum:]_.:/-]+::[[:alnum:]_.:/-]+|gpu/compute::[[:alnum:]_.:/-]+|editor/[[:alnum:]_.:/-]+::[[:alnum:]_.:/-]+)\)' "${PRELUDE_FILES[@]}"
  fi
}

extract_runner_ops() {
  if command -v rg >/dev/null 2>&1; then
    rg -o --no-filename --pcre2 '"(browser/[[:alnum:]_.:/-]+::[[:alnum:]_.:/-]+|gfx/[[:alnum:]_.:/-]+::[[:alnum:]_.:/-]+|gpu/compute::[[:alnum:]_.:/-]+|editor/[[:alnum:]_.:/-]+::[[:alnum:]_.:/-]+)"' "${RUNNER_FILES[@]}"
  else
    grep -Eho '"(browser/[[:alnum:]_.:/-]+::[[:alnum:]_.:/-]+|gfx/[[:alnum:]_.:/-]+::[[:alnum:]_.:/-]+|gpu/compute::[[:alnum:]_.:/-]+|editor/[[:alnum:]_.:/-]+::[[:alnum:]_.:/-]+)"' "${RUNNER_FILES[@]}"
  fi
}

extract_wrapper_ops \
  | sed -E 's/^\(quote[[:space:]]+//; s/\)$//' \
  | sort -u >"$WRAPPER_OPS"

if [[ ! -s "$WRAPPER_OPS" ]]; then
  echo "prelude-capability-coverage: no browser/gfx/gpu-compute/editor capability wrapper ops found"
  exit 1
fi

extract_runner_ops \
  | tr -d '"' \
  | sort -u >"$RUNNER_OPS"

if [[ ! -s "$RUNNER_OPS" ]]; then
  echo "prelude-capability-coverage: no browser/gfx/gpu-compute/editor ops found in runtime dispatch"
  exit 1
fi

comm -23 "$WRAPPER_OPS" "$RUNNER_OPS" >"$MISSING" || true
if [[ -s "$MISSING" ]]; then
  echo "prelude-capability-coverage: runner is missing prelude wrapper ops:"
  cat "$MISSING"
  exit 1
fi

echo "prelude-capability-coverage: ok"
