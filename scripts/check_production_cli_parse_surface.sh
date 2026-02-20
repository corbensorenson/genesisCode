#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

CAPS="$TMP/caps.toml"
cat >"$CAPS" <<'TOML'
allow = ["core/refs::list"]

[store]
dir = "./.genesis/store"

[refs]
path = "./.genesis/refs.gc"
TOML

assert_rejects_rust() {
  local bin="$1"
  local err="$TMP/${bin}.err"
  set +e
  cargo run --release -q -p "$2" --bin "$bin" -- \
    --coreform-frontend rust \
    refs \
    --caps "$CAPS" \
    list >"$TMP/${bin}.out" 2>"$err"
  local code=$?
  set -e
  if [[ $code -eq 0 ]]; then
    echo "parse-surface: expected $bin to reject --coreform-frontend rust"
    cat "$TMP/${bin}.out"
    cat "$err"
    exit 1
  fi
  if [[ $code -ne 2 ]]; then
    echo "parse-surface: expected exit code 2 from $bin, got $code"
    cat "$err"
    exit 1
  fi
  grep -Fq "invalid value 'rust' for '--coreform-frontend <COREFORM_FRONTEND>'" "$err" || {
    echo "parse-surface: $bin stderr missing parse rejection detail"
    cat "$err"
    exit 1
  }
  grep -Fq 'expected `selfhost`' "$err" || {
    echo "parse-surface: $bin stderr missing expected selfhost hint"
    cat "$err"
    exit 1
  }
}

assert_accepts_rust_parity() {
  local bin="$1"
  cargo run --release -q -p "$2" --bin "$bin" -- \
    --coreform-frontend rust \
    refs \
    --caps "$CAPS" \
    list >/dev/null
}

assert_rejects_rust genesis gc_cli
assert_rejects_rust genesis_wasi gc_wasi_cli
assert_accepts_rust_parity genesis_parity gc_cli
assert_accepts_rust_parity genesis_wasi_parity gc_wasi_cli

echo "parse-surface: ok"
