#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

SEMANTIC_TOKEN_REGEX='parse_module\(|canonicalize_module\(|print_module\(|hash_module\(|eval_module\(|eval_term\('

is_allowed_path() {
  local p="$1"
  case "$p" in
    crates/gc_coreform/*) return 0 ;;
    crates/gc_kernel/*) return 0 ;;
    crates/gc_prelude/*) return 0 ;;
    crates/gc_opt/*) return 0 ;;
    crates/gc_types/*) return 0 ;;
    crates/gc_patches/*) return 0 ;;
    crates/gc_obligations/*) return 0 ;;
    crates/gc_effects/src/runner*.rs) return 0 ;;
    crates/gc_cli/src/main.rs) return 0 ;;
    crates/gc_wasi_cli/src/main.rs) return 0 ;;
    crates/gc_wasm/src/lib.rs) return 0 ;;
    crates/*/tests/*) return 0 ;;
    *) return 1 ;;
  esac
}

resolve_base() {
  if [[ -n "${SELFHOST_BOUNDARY_BASE:-}" ]]; then
    echo "$SELFHOST_BOUNDARY_BASE"
    return 0
  fi

  if [[ -n "${GITHUB_BASE_REF:-}" ]]; then
    git fetch --no-tags --depth=1 origin "$GITHUB_BASE_REF" >/dev/null 2>&1 || true
    local mb
    mb="$(git merge-base HEAD "origin/${GITHUB_BASE_REF}" 2>/dev/null || true)"
    if [[ -n "$mb" ]]; then
      echo "$mb"
      return 0
    fi
  fi

  if git rev-parse HEAD~1 >/dev/null 2>&1; then
    echo "HEAD~1"
    return 0
  fi

  echo ""
}

BASE_REF="$(resolve_base)"
if [[ -z "$BASE_REF" ]]; then
  echo "selfhost-boundary: no diff base detected; skipping guard."
  exit 0
fi

CHANGED_RS="$(git diff --name-only "$BASE_REF"...HEAD -- 'crates/**/*.rs')"

if [[ -z "$CHANGED_RS" ]]; then
  echo "selfhost-boundary: no changed Rust files under crates/."
  exit 0
fi

violations=0

while IFS= read -r file; do
  [[ -n "$file" ]] || continue
  [[ -f "$file" ]] || continue

  if is_allowed_path "$file"; then
    continue
  fi

  if git diff -U0 "$BASE_REF"...HEAD -- "$file" \
    | grep -E '^\+' \
    | grep -Eq "$SEMANTIC_TOKEN_REGEX"; then
    echo "selfhost-boundary violation: semantic token added in non-approved file: $file"
    violations=$((violations + 1))
  fi
done <<EOF
$CHANGED_RS
EOF

if [[ "$violations" -gt 0 ]]; then
  cat <<'EOF'
selfhost-boundary: failed.
Do not add new Rust language-semantic surface outside approved modules.
Move semantic logic into .gc toolchain modules and keep Rust as host/runtime boundary.
EOF
  exit 1
fi

echo "selfhost-boundary: ok"
