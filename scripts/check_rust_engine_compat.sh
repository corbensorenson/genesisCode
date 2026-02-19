#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

find_candidates() {
  rg --files \
    crates/gc_cli/tests \
    crates/gc_wasi_cli/tests \
    scripts \
    .github/workflows \
    | sort
}

has_rust_engine_usage() {
  local file="$1"
  rg -q \
    -e "--engine rust" \
    -e "coreform-frontend rust" \
    -e "engine\\\", \\\"rust\\\"" \
    "$file"
}

has_parity_harness_usage() {
  local file="$1"
  rg -q "genesis_parity|genesis_wasi_parity" "$file"
}

violations=0

while IFS= read -r file; do
  [[ -n "$file" ]] || continue
  [[ -f "$file" ]] || continue

  if ! has_rust_engine_usage "$file"; then
    continue
  fi

  if rg -q "RUST_ENGINE_COMPAT_EXCEPTION" "$file"; then
    continue
  fi
  if has_parity_harness_usage "$file"; then
    continue
  fi

  echo "rust-engine compat violation: $file uses rust-engine/frontend without parity harness binary (genesis_parity/genesis_wasi_parity) or explicit RUST_ENGINE_COMPAT_EXCEPTION"
  violations=$((violations + 1))
done < <(find_candidates)

if [[ "$violations" -gt 0 ]]; then
  cat <<'EOF'
rust-engine compat guard: failed.
Rust engine/frontend paths are parity-only and must run through dedicated parity binaries
(`genesis_parity` / `genesis_wasi_parity`) unless explicitly exempted.
EOF
  exit 1
fi

echo "rust-engine compat guard: ok"
