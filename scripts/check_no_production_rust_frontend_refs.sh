#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

TARGETS=(
  "crates/gc_cli_driver/src/cmd_pkg.rs"
  "crates/gc_cli_driver/src/cmd_gc.rs"
  "crates/gc_cli_driver/src/cmd_refs.rs"
  "crates/gc_cli_driver/src/cmd_sync.rs"
  "crates/gc_cli_driver/src/cmd_vcs.rs"
  "crates/gc_obligations/src/lib.rs"
)

PATTERN='CoreformFrontend::Rust'
violations=0

for file in "${TARGETS[@]}"; do
  if [[ ! -f "$file" ]]; then
    echo "no-production-rust-frontend-refs: missing expected file: $file" >&2
    violations=1
    continue
  fi
  if rg -n "$PATTERN" "$file" >/tmp/genesis_rust_frontend_refs.$$ 2>/dev/null; then
    echo "no-production-rust-frontend-refs: found forbidden rust-frontend reference(s) in $file" >&2
    cat /tmp/genesis_rust_frontend_refs.$$ >&2
    violations=1
  fi
done

rm -f /tmp/genesis_rust_frontend_refs.$$ || true

if [[ "$violations" -ne 0 ]]; then
  exit 1
fi

echo "no-production-rust-frontend-refs: ok"
