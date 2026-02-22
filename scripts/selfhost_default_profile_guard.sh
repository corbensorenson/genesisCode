#!/usr/bin/env bash
set -euo pipefail

# RUST_ENGINE_COMPAT_EXCEPTION:
# This guard intentionally invokes `--engine rust` to assert default-profile rejection.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "selfhost-default-profile-guard" \
  ".genesis/build/cargo" \
  "GENESIS_SELFHOST_DEFAULT_PROFILE_GUARD_CARGO_TARGET_DIR"

DEFAULT_DEBUG_DIR="$CARGO_TARGET_DIR/debug"
GEN="${GEN:-$DEFAULT_DEBUG_DIR/genesis}"
GWASI="${GWASI:-$DEFAULT_DEBUG_DIR/genesis_wasi}"

if [[ ! -x "$GEN" ]]; then
  cargo build -p gc_cli >/dev/null
fi
if [[ ! -x "$GWASI" ]]; then
  cargo build -p gc_wasi_cli >/dev/null
fi

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

ART="$TMP_DIR/selfhost_toolchain.gc"
REPO_ART="$ROOT_DIR/selfhost/toolchain.gc"
if [[ -f "$REPO_ART" ]]; then
  cp "$REPO_ART" "$ART"
else
  "$GEN" selfhost-artifact --out "$ART" >/dev/null
fi

cat >"$TMP_DIR/mod.gc" <<'GC'
(def m::x (prim int/add 1 2))
m::x
GC

cat >"$TMP_DIR/caps.toml" <<'TOML'
allow = []
TOML

cat >"$TMP_DIR/prog.gc" <<'GC'
(def prog (core/effect::pure 7))
prog
GC

fail() {
  echo "selfhost-default-profile-guard: $*" >&2
  exit 1
}

expect_rust_engine_rejected() {
  local label="$1"
  shift
  set +e
  local out
  out="$("$@" 2>&1)"
  local code=$?
  set -e
  [[ "$code" == "2" ]] || fail "$label expected exit 2, got $code (out=$out)"
  [[ "$out" == *"invalid value 'rust'"* && "$out" == *"expected"* && "$out" == *"selfhost"* ]] || {
    fail "$label missing parse-level rust rejection (out=$out)"
  }
}

# Native: default profile rejects rust engine, selfhost default path still works.
expect_rust_engine_rejected "native fmt --engine rust" \
  "$GEN" fmt "$TMP_DIR/mod.gc" --engine rust
expect_rust_engine_rejected "native eval --engine rust" \
  "$GEN" eval "$TMP_DIR/mod.gc" --engine rust
expect_rust_engine_rejected "native run --engine rust" \
  "$GEN" run "$TMP_DIR/prog.gc" --engine rust --caps "$TMP_DIR/caps.toml" --log "$TMP_DIR/n.rust.gclog"

n_eval="$("$GEN" --selfhost-artifact "$ART" eval "$TMP_DIR/mod.gc" | tr -d '\n')"
[[ "$n_eval" == "3" ]] || fail "native eval default selfhost produced unexpected output: $n_eval"

# WASI native binary: same guarantees.
expect_rust_engine_rejected "wasi fmt --engine rust" \
  "$GWASI" fmt "$TMP_DIR/mod.gc" --engine rust
expect_rust_engine_rejected "wasi eval --engine rust" \
  "$GWASI" eval "$TMP_DIR/mod.gc" --engine rust
expect_rust_engine_rejected "wasi run --engine rust" \
  "$GWASI" run "$TMP_DIR/prog.gc" --engine rust --caps "$TMP_DIR/caps.toml" --log "$TMP_DIR/w.rust.gclog"

w_eval="$("$GWASI" --selfhost-artifact "$ART" eval "$TMP_DIR/mod.gc" | tr -d '\n')"
[[ "$w_eval" == "3" ]] || fail "wasi eval default selfhost produced unexpected output: $w_eval"

echo "selfhost-default-profile-guard: ok"
