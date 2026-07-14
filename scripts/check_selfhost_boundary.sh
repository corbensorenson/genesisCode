#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

SEMANTIC_TOKEN_REGEX='parse_module\(|canonicalize_module\(|print_module\(|hash_module\(|eval_module\(|eval_term\('
MODE="${SELFHOST_BOUNDARY_MODE:-diff}"

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
    crates/gc_cli_driver/src/cmd_*.rs) return 0 ;;
    crates/gc_cli_driver/src/selfhost_bridge.rs) return 0 ;;
    crates/gc_cli_driver/src/pkg_self_opt.rs) return 0 ;;
    crates/gc_cli_driver/src/kernel_exec.rs) return 0 ;;
    crates/gc_effects/src/lib.rs) return 0 ;;
    crates/gc_effects/src/runner*.rs) return 0 ;;
    crates/*/src/tests.rs) return 0 ;;
    crates/*/src/tests_*.rs) return 0 ;;
    crates/gc_cli/src/main.rs) return 0 ;;
    crates/gc_wasi_cli/src/main.rs) return 0 ;;
    crates/gc_wasm/src/lib.rs) return 0 ;;
    crates/gc_wasm/src/runtime.rs) return 0 ;;
    crates/*/tests/*) return 0 ;;
    *) return 1 ;;
  esac
}

usage() {
  cat <<'EOF'
Usage: scripts/check_selfhost_boundary.sh [--diff|--strict]

Modes:
  --diff    inspect only semantic tokens added in diff lines (default)
  --strict  inspect full Rust source tree for semantic tokens in non-approved files
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --diff)
      MODE="diff"
      shift
      ;;
    --strict)
      MODE="strict"
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "selfhost-boundary: unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [[ "$MODE" != "diff" && "$MODE" != "strict" ]]; then
  echo "selfhost-boundary: invalid mode '$MODE' (expected diff|strict)" >&2
  exit 2
fi

resolve_base() {
  if [[ -n "${SELFHOST_BOUNDARY_BASE:-}" ]]; then
    echo "$SELFHOST_BOUNDARY_BASE"
    return 0
  fi

  if [[ -n "${GITHUB_BASE_REF:-}" ]]; then
    if declare -F genesis_gate_telemetry_event >/dev/null 2>&1; then
      genesis_gate_telemetry_event network-attempt 1
    fi
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

check_added_semantic_tokens() {
  local base_ref="$1"
  local file="$2"
  git diff -U0 "$base_ref"...HEAD -- "$file" \
    | grep -E '^\+' \
    | grep -Eq "$SEMANTIC_TOKEN_REGEX"
}

check_full_file_semantic_tokens() {
  local file="$1"
  grep -Eq "$SEMANTIC_TOKEN_REGEX" "$file"
}

list_production_rust_files() {
  if command -v rg >/dev/null 2>&1; then
    rg --files crates --glob 'crates/*/src/**/*.rs' \
      | grep -v '^crates/gc_runtime_bench/' \
      | sort
    return 0
  fi
  find crates -type f -name '*.rs' \
    | grep '/src/' \
    | grep -v '^crates/gc_runtime_bench/' \
    | sort
}

print_semantic_matches() {
  local file="$1"
  if command -v rg >/dev/null 2>&1; then
    rg -n "$SEMANTIC_TOKEN_REGEX" "$file" | sed -n '1,5p'
    return 0
  fi
  grep -En "$SEMANTIC_TOKEN_REGEX" "$file" | sed -n '1,5p'
}

FILES_TO_SCAN=""
BASE_REF=""
if [[ "$MODE" == "strict" ]]; then
  FILES_TO_SCAN="$(list_production_rust_files)"
  if [[ -z "$FILES_TO_SCAN" ]]; then
    echo "selfhost-boundary: no Rust files under crates/."
    exit 0
  fi
else
  BASE_REF="$(resolve_base)"
  if [[ -z "$BASE_REF" ]]; then
    echo "selfhost-boundary: no diff base detected; escalating to strict mode."
    MODE="strict"
    FILES_TO_SCAN="$(list_production_rust_files)"
    if [[ -z "$FILES_TO_SCAN" ]]; then
      echo "selfhost-boundary: no Rust files under crates/."
      exit 0
    fi
  else
    FILES_TO_SCAN="$(git diff --name-only "$BASE_REF"...HEAD -- 'crates/**/*.rs')"
    if [[ -z "$FILES_TO_SCAN" ]]; then
      echo "selfhost-boundary: no changed Rust files under crates/."
      exit 0
    fi
  fi
fi

violations=0

while IFS= read -r file; do
  [[ -n "$file" ]] || continue
  [[ -f "$file" ]] || continue

  if is_allowed_path "$file"; then
    continue
  fi

  if [[ "$MODE" == "strict" ]]; then
    if check_full_file_semantic_tokens "$file"; then
      echo "selfhost-boundary violation (strict): semantic token in non-approved file: $file"
      print_semantic_matches "$file"
      violations=$((violations + 1))
    fi
  else
    if check_added_semantic_tokens "$BASE_REF" "$file"; then
      echo "selfhost-boundary violation (diff): semantic token added in non-approved file: $file"
      violations=$((violations + 1))
    fi
  fi
done <<EOF
$FILES_TO_SCAN
EOF

if [[ "$violations" -gt 0 ]]; then
  cat <<'EOF'
selfhost-boundary: failed.
Do not add new Rust language-semantic surface outside approved modules.
Move semantic logic into .gc toolchain modules and keep Rust as host/runtime boundary.
EOF
  exit 1
fi

echo "selfhost-boundary: ok (mode=$MODE)"
