#!/usr/bin/env bash
set -euo pipefail
export LANG=C
export LC_ALL=C

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "selfhost-typecheck-authority" \
  root-host
TARGET_DIR="$CARGO_TARGET_DIR"

cargo build --locked --offline \
  -p gc_cli --bin genesis \
  -p gc_wasi_cli --bin genesis_wasi \
  >/dev/null
cargo build --locked --offline \
  -p gc_cli --features parity-harness --bin genesis_parity \
  >/dev/null

python3 scripts/lib/selfhost_typecheck_authority.py \
  --root "$ROOT_DIR" \
  --profile policies/selfhost_typecheck_authority_v0.1.json \
  --schema docs/spec/SELFHOST_TYPECHECK_AUTHORITY_v0.1.schema.json \
  --self-test \
  --runtime \
  --genesis-bin "$TARGET_DIR/debug/genesis" \
  --genesis-wasi-bin "$TARGET_DIR/debug/genesis_wasi"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT
CORPUS_DIR="$TMP_DIR/corpus"
mkdir -p "$CORPUS_DIR"
CORPUS_MANIFEST="$TMP_DIR/corpus.manifest"
CASE_COUNT=0
MISMATCHES=()

for package in "$ROOT_DIR"/tests/spec/pkg_*/package.toml; do
  fixture_dir="$(dirname "$package")"
  fixture_name="$(basename "$fixture_dir")"
  copied_dir="$CORPUS_DIR/$fixture_name"
  cp -R "$fixture_dir" "$copied_dir"
  copied_package="$copied_dir/package.toml"
  rust_out="$TMP_DIR/$fixture_name.rust.json"
  selfhost_out="$TMP_DIR/$fixture_name.selfhost.json"

  while IFS= read -r fixture_file; do
    relative="${fixture_file#"$copied_dir/"}"
    printf '%s/%s  ' "$fixture_name" "$relative" >>"$CORPUS_MANIFEST"
    shasum -a 256 "$fixture_file" | awk '{print $1}' >>"$CORPUS_MANIFEST"
  done < <(find "$copied_dir" -type f | LC_ALL=C sort)

  set +e
  "$TARGET_DIR/debug/genesis_parity" \
    --step-limit 50000000 \
    --max-alloc-units 50000000 \
    --coreform-frontend rust \
    --json typecheck --pkg "$copied_package" \
    >"$rust_out" 2>&1
  rust_exit=$?
  "$TARGET_DIR/debug/genesis" \
    --step-limit 50000000 \
    --max-alloc-units 50000000 \
    --selfhost-only \
    --selfhost-artifact "$ROOT_DIR/selfhost/toolchain.gc" \
    --coreform-frontend selfhost \
    --json typecheck --pkg "$copied_package" \
    >"$selfhost_out" 2>&1
  selfhost_exit=$?
  set -e

  if [[ "$rust_exit" -ne "$selfhost_exit" ]]; then
    MISMATCHES+=("$fixture_name:exit:$rust_exit:$selfhost_exit")
  fi
  if ! python3 - "$rust_out" "$selfhost_out" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    rust = json.load(handle)
with open(sys.argv[2], encoding="utf-8") as handle:
    selfhost = json.load(handle)

def semantic_projection(value):
    data = value.get("data")
    if not isinstance(data, dict):
        data = {}
    return {
        "diagnostics": value.get("diagnostics"),
        "kind": value.get("kind"),
        "ok": value.get("ok"),
        "report_coreform": data.get("report_coreform"),
    }

raise SystemExit(semantic_projection(rust) != semantic_projection(selfhost))
PY
  then
    MISMATCHES+=("$fixture_name:semantic-report")
  fi
  CASE_COUNT=$((CASE_COUNT + 1))
done

if [[ "$CASE_COUNT" -ne 23 ]]; then
  echo "selfhost-typecheck-authority: expected 23 corpus fixtures, observed $CASE_COUNT" >&2
  exit 1
fi
if [[ "${#MISMATCHES[@]}" -ne 0 ]]; then
  printf 'selfhost-typecheck-authority: corpus mismatches: %s\n' "${MISMATCHES[*]}" >&2
  exit 1
fi
CORPUS_IDENTITY="$(shasum -a 256 "$CORPUS_MANIFEST" | awk '{print $1}')"
printf '{"caseCount":%d,"contentIdentitySha256":"%s","kind":"genesis/selfhost-typecheck-corpus-check-v0.1","ok":true}\n' \
  "$CASE_COUNT" "$CORPUS_IDENTITY"
