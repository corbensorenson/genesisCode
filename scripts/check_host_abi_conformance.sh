#!/usr/bin/env bash
set -euo pipefail

export LC_ALL=C

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

RUNNER_FILES=(
  "crates/gc_effects/src/runner_capability_dispatch.rs"
  "crates/gc_effects/src/runner_cap_pkg_low.rs"
  "crates/gc_effects/src/runner_cap_vcs_low.rs"
  "crates/gc_effects/src/runner_cap_gc_gpk_low.rs"
)
DOC_FILE="docs/spec/HOST_ABI.md"

for f in "${RUNNER_FILES[@]}"; do
  if [[ ! -f "$f" ]]; then
    echo "host-abi-conformance: missing dispatch file: $f"
    exit 1
  fi
done
if [[ ! -f "$DOC_FILE" ]]; then
  echo "host-abi-conformance: missing doc file: $DOC_FILE"
  exit 1
fi

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

IMPL_SORTED="$TMP_DIR/impl_sorted.txt"
DOC_RAW="$TMP_DIR/doc_raw.txt"
DOC_SORTED="$TMP_DIR/doc_sorted.txt"

extract_impl_ops() {
  if command -v rg >/dev/null 2>&1; then
    rg -o --no-filename --pcre2 '"([[:alnum:]_/-]+::[[:alnum:]_/:.-]+)"' "${RUNNER_FILES[@]}"
  else
    grep -Eho '"[[:alnum:]_/-]+::[[:alnum:]_/:.-]+"' "${RUNNER_FILES[@]}"
  fi
}

extract_impl_ops \
  | tr -d '"' \
  | sort -u >"$IMPL_SORTED"

awk '
  /HOST_ABI_OPS_BEGIN/ { in_doc = 1; next; }
  /HOST_ABI_OPS_END/ { in_doc = 0; next; }
  in_doc {
    if (match($0, /`[^`]+::[^`]+`/)) {
      line = substr($0, RSTART + 1, RLENGTH - 2);
      print line;
    }
  }
' "$DOC_FILE" >"$DOC_RAW"

if [[ ! -s "$DOC_RAW" ]]; then
  echo "host-abi-conformance: no documented ops found between HOST_ABI_OPS markers"
  exit 1
fi
if [[ ! -s "$IMPL_SORTED" ]]; then
  echo "host-abi-conformance: no implementation ops detected in capability dispatch"
  exit 1
fi

sort -u "$DOC_RAW" >"$DOC_SORTED"

if ! cmp -s "$DOC_RAW" "$DOC_SORTED"; then
  echo "host-abi-conformance: documented host ABI ops must be globally sorted and unique"
  echo "expected sorted unique list:"
  cat "$DOC_SORTED"
  echo "actual list:"
  cat "$DOC_RAW"
  exit 1
fi

if ! diff -u "$DOC_SORTED" "$IMPL_SORTED" >/dev/null; then
  echo "host-abi-conformance: documented and implemented host ABI op surfaces differ"
  diff -u "$DOC_SORTED" "$IMPL_SORTED" || true
  exit 1
fi

echo "host-abi-conformance: ok"
