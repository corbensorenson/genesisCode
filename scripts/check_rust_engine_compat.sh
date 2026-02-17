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
  rg -q -e "--engine rust" -e "engine\\\", \\\"rust\\\"" "$file"
}

violations=0

while IFS= read -r file; do
  [[ -n "$file" ]] || continue
  [[ -f "$file" ]] || continue

  if ! has_rust_engine_usage "$file"; then
    continue
  fi

  case "$file" in
    *.rs)
      if rg -q "RUST_ENGINE_COMPAT_EXCEPTION" "$file"; then
        continue
      fi
      if ! rg -q "GENESIS_ALLOW_RUST_ENGINE" "$file"; then
        echo "rust-engine compat violation: $file uses --engine rust without explicit GENESIS_ALLOW_RUST_ENGINE opt-in"
        violations=$((violations + 1))
      fi
      ;;
    *.sh|*.yml|*.yaml)
      if rg -q "RUST_ENGINE_COMPAT_EXCEPTION" "$file"; then
        continue
      fi
      if ! rg -q "GENESIS_ALLOW_RUST_ENGINE=1|GENESIS_ALLOW_RUST_ENGINE:\\s*\"?1\"?" "$file"; then
        echo "rust-engine compat violation: $file uses --engine rust without explicit GENESIS_ALLOW_RUST_ENGINE=1 opt-in"
        violations=$((violations + 1))
      fi
      ;;
    *)
      echo "rust-engine compat violation: unsupported file type with --engine rust usage: $file"
      violations=$((violations + 1))
      ;;
  esac
done < <(find_candidates)

if [[ "$violations" -gt 0 ]]; then
  cat <<'EOF'
rust-engine compat guard: failed.
All rust-engine usages must declare explicit compatibility mode via GENESIS_ALLOW_RUST_ENGINE=1.
Default profile must remain selfhost-first and rust-engine-free.
EOF
  exit 1
fi

echo "rust-engine compat guard: ok"
