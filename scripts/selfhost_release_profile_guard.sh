#!/usr/bin/env bash
set -euo pipefail

# RUST_ENGINE_COMPAT_EXCEPTION:
# This guard intentionally invokes `--engine rust` / `--coreform-frontend rust`
# to prove release binaries reject compatibility-mode execution unconditionally.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

GEN="${GEN_REL:-$ROOT_DIR/target/release/genesis}"
GWASI="${GWASI_REL:-$ROOT_DIR/target/release/genesis_wasi}"

if [[ ! -x "$GEN" ]]; then
  cargo build -p gc_cli --release >/dev/null
fi
if [[ ! -x "$GWASI" ]]; then
  cargo build -p gc_wasi_cli --release >/dev/null
fi

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

cat >"$TMP_DIR/mod.gc" <<'GC'
(def m::x (prim int/add 1 2))
m::x
GC

cat >"$TMP_DIR/prog.gc" <<'GC'
(def prog (core/effect::pure 7))
prog
GC

cat >"$TMP_DIR/caps.toml" <<'TOML'
allow = []
TOML

cat >"$TMP_DIR/policies.toml" <<'TOML'
version = 1
default = "policy:default-v0.1"
TOML

fail() {
  echo "selfhost-release-profile-guard: $*" >&2
  exit 1
}

expect_release_rejected() {
  local label="$1"
  shift
  set +e
  local out
  out="$("$@" 2>&1)"
  local code=$?
  set -e
  [[ "$code" == "50" ]] || fail "$label expected exit 50, got $code (out=$out)"
  [[ "$out" == *"disabled in production binaries"* ]] || {
    fail "$label missing release-profile rejection message (out=$out)"
  }
}

expect_bootstrap_mode_rejected() {
  local label="$1"
  shift
  set +e
  local out
  out="$("$@" 2>&1)"
  local code=$?
  set -e
  [[ "$code" == "50" ]] || fail "$label expected exit 50, got $code (out=$out)"
  [[ "$out" == *"--selfhost-bootstrap artifact-only"* ]] || {
    fail "$label missing release bootstrap-mode rejection message (out=$out)"
  }
}

# Native release binary: rust compatibility must remain disabled regardless of env flag.
expect_release_rejected "native release fmt --engine rust" \
  "$GEN" fmt "$TMP_DIR/mod.gc" --engine rust
expect_release_rejected "native release eval --engine rust" \
  "$GEN" eval "$TMP_DIR/mod.gc" --engine rust
expect_release_rejected "native release run --engine rust" \
  "$GEN" run "$TMP_DIR/prog.gc" --engine rust --caps "$TMP_DIR/caps.toml" --log "$TMP_DIR/native.rust.gclog"
expect_release_rejected "native release policy list --coreform-frontend rust" \
  "$GEN" --coreform-frontend rust policy list --policies "$TMP_DIR/policies.toml"
expect_bootstrap_mode_rejected "native release fmt --selfhost-bootstrap embedded" \
  "$GEN" fmt "$TMP_DIR/mod.gc" --selfhost-bootstrap embedded

# Native WASI-host binary: same guarantees.
expect_release_rejected "wasi release fmt --engine rust" \
  "$GWASI" fmt "$TMP_DIR/mod.gc" --engine rust
expect_release_rejected "wasi release eval --engine rust" \
  "$GWASI" eval "$TMP_DIR/mod.gc" --engine rust
expect_release_rejected "wasi release run --engine rust" \
  "$GWASI" run "$TMP_DIR/prog.gc" --engine rust --caps "$TMP_DIR/caps.toml" --log "$TMP_DIR/wasi.rust.gclog"
expect_release_rejected "wasi release policy list --coreform-frontend rust" \
  "$GWASI" --coreform-frontend rust policy list --policies "$TMP_DIR/policies.toml"
expect_bootstrap_mode_rejected "wasi release fmt --selfhost-bootstrap embedded" \
  "$GWASI" fmt "$TMP_DIR/mod.gc" --selfhost-bootstrap embedded

echo "selfhost-release-profile-guard: ok"
