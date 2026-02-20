#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

DOC_PATH="docs/spec/BOOTSTRAP_OLD.md"
violations=0

tree_has_pattern() {
  local pattern="$1"
  if command -v rg >/dev/null 2>&1; then
    rg -n --glob '*.rs' "$pattern" crates >/dev/null
  else
    grep -Rns --include '*.rs' -E "$pattern" crates >/dev/null
  fi
}

print_tree_matches() {
  local pattern="$1"
  if command -v rg >/dev/null 2>&1; then
    rg -n --glob '*.rs' "$pattern" crates || true
  else
    grep -Rns --include '*.rs' -E "$pattern" crates || true
  fi
}

doc_has_pattern() {
  local pattern="$1"
  if command -v rg >/dev/null 2>&1; then
    rg -q "$pattern" "$DOC_PATH"
  else
    grep -Eq "$pattern" "$DOC_PATH"
  fi
}

if [[ ! -f "$DOC_PATH" ]]; then
  echo "old-bootstrap retirement guard: missing $DOC_PATH"
  exit 1
fi

if tree_has_pattern 'old_bootstrap/rust_semantics|legacy_program_builders'; then
  echo "old-bootstrap retirement violation: active crates still reference archived rust_semantics"
  print_tree_matches 'old_bootstrap/rust_semantics|legacy_program_builders'
  violations=$((violations + 1))
fi

if ! doc_has_pattern '^Cutover Status: APPROVED$'; then
  echo "old-bootstrap retirement violation: $DOC_PATH must declare 'Cutover Status: APPROVED'"
  violations=$((violations + 1))
fi

if ! doc_has_pattern '^Approval Date: [0-9]{4}-[0-9]{2}-[0-9]{2}$'; then
  echo "old-bootstrap retirement violation: $DOC_PATH must declare an ISO approval date"
  violations=$((violations + 1))
fi

if ! doc_has_pattern '^Approver: .+$'; then
  echo "old-bootstrap retirement violation: $DOC_PATH must declare a non-empty approver"
  violations=$((violations + 1))
fi

if awk '
  /^## Rust-to-old_bootstrap Retirement Gate$/ { in_gate=1; next }
  /^## / && in_gate==1 { in_gate=0 }
  in_gate==1 { print }
' "$DOC_PATH" | grep -Eq '^- \[ \] '; then
  echo "old-bootstrap retirement violation: gate checklist in $DOC_PATH has unchecked items"
  violations=$((violations + 1))
fi

if [[ "$violations" -gt 0 ]]; then
  cat <<'EOF'
old-bootstrap retirement guard: failed.
Release/runtime paths must be fully detached from archived bootstrap semantics,
and the retirement gate checklist must remain explicitly approved.
EOF
  exit 1
fi

echo "old-bootstrap retirement guard: ok"
