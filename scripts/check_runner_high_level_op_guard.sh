#!/usr/bin/env bash
set -euo pipefail

export LC_ALL=C

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

RUNNER_FILE="crates/gc_effects/src/runner.rs"
ALLOWLIST_FILE="docs/spec/RUNNER_HIGH_LEVEL_OPS_ALLOWLIST_v0.2.txt"

if [[ ! -f "$RUNNER_FILE" ]]; then
  echo "runner-high-level-op-guard: missing runner file: $RUNNER_FILE"
  exit 1
fi
if [[ ! -f "$ALLOWLIST_FILE" ]]; then
  echo "runner-high-level-op-guard: missing allowlist file: $ALLOWLIST_FILE"
  exit 1
fi

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

IMPL_ALL="$TMP_DIR/impl_all.txt"
IMPL_HIGH="$TMP_DIR/impl_high.txt"
ALLOWLIST_SORTED="$TMP_DIR/allowlist_sorted.txt"
UNEXPECTED="$TMP_DIR/unexpected.txt"

awk '
  /^fn call_capability\(/ { in_fn = 1; }
  in_fn && /match op(_eff)? \{/ { in_match = 1; next; }
  in_match {
    if ($0 ~ /^        _[[:space:]]*=>/) {
      in_match = 0;
      in_fn = 0;
    }
    if ($0 ~ /^        "[[:alnum:]\/:_-]+::[[:alnum:]\/:_-]+"[[:space:]]*=>/ || $0 ~ /^        "[[:alnum:]\/:_-]+::[[:alnum:]\/:_-]+"[[:space:]]*$/ || $0 ~ /^        \|[[:space:]]*"[[:alnum:]\/:_-]+::[[:alnum:]\/:_-]+"/) {
      line = $0;
      sub(/^[[:space:]]*\|?[[:space:]]*"/, "", line);
      sub(/".*$/, "", line);
      print line;
    }
  }
' "$RUNNER_FILE" | sort -u >"$IMPL_ALL"

if [[ ! -s "$IMPL_ALL" ]]; then
  echo "runner-high-level-op-guard: no capability ops found in runner dispatch"
  exit 1
fi

grep -E '^core/(pkg|vcs|gc|gpk)::' "$IMPL_ALL" | sort -u >"$IMPL_HIGH" || true
sort -u "$ALLOWLIST_FILE" >"$ALLOWLIST_SORTED"

if ! cmp -s "$ALLOWLIST_FILE" "$ALLOWLIST_SORTED"; then
  echo "runner-high-level-op-guard: allowlist must be globally sorted and unique"
  echo "expected:"
  cat "$ALLOWLIST_SORTED"
  echo "actual:"
  cat "$ALLOWLIST_FILE"
  exit 1
fi

comm -23 "$IMPL_HIGH" "$ALLOWLIST_SORTED" >"$UNEXPECTED" || true

if [[ -s "$UNEXPECTED" ]]; then
  echo "runner-high-level-op-guard: unexpected high-level semantic op(s) in runner dispatch:"
  cat "$UNEXPECTED"
  cat <<'EOF'
This guard prevents semantic creep back into Rust while extraction to .gc is in progress.
If this is intentional, update docs/spec/RUNNER_HIGH_LEVEL_OPS_ALLOWLIST_v0.2.txt in the same change.
EOF
  exit 1
fi

echo "runner-high-level-op-guard: ok"
