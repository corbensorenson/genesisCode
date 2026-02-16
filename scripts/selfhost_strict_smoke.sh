#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

native() {
  cargo run -p gc_cli --quiet -- "$@"
}

wasi_native() {
  cargo run -p gc_wasi_cli --quiet -- "$@"
}

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

ART="$TMP_DIR/selfhost_toolchain.gc"
native selfhost-artifact --out "$ART" >/dev/null

# fmt/eval/optimize strict selfhost smoke (native CLI)
cat >"$TMP_DIR/mod.gc" <<'GC'
(def m::x (prim int/add 1 2))
m::x
GC

native --selfhost-only --selfhost-artifact "$ART" fmt "$TMP_DIR/mod.gc" >/dev/null
native --selfhost-only --selfhost-artifact "$ART" fmt "$TMP_DIR/mod.gc" --check >/dev/null

N_EVAL="$(native eval "$TMP_DIR/mod.gc" | tr -d '\n')"
S_EVAL="$(native --selfhost-only --selfhost-artifact "$ART" eval "$TMP_DIR/mod.gc" | tr -d '\n')"
if [[ "$N_EVAL" != "$S_EVAL" ]]; then
  echo "native strict eval mismatch native=$N_EVAL strict=$S_EVAL" >&2
  exit 1
fi

N_OPT="$TMP_DIR/opt.native.gc"
S_OPT="$TMP_DIR/opt.strict.gc"
native optimize "$TMP_DIR/mod.gc" --out "$N_OPT" >/dev/null
native --selfhost-only --selfhost-artifact "$ART" optimize "$TMP_DIR/mod.gc" --out "$S_OPT" >/dev/null
if ! diff -u "$N_OPT" "$S_OPT" >/dev/null; then
  echo "native strict optimize output mismatch" >&2
  exit 1
fi

# package strict selfhost smoke (native CLI)
PKG_N="$TMP_DIR/pkg_native"
mkdir -p "$PKG_N"
cp tests/spec/pkg_basic/basic.gc "$PKG_N/basic.gc"
cp tests/spec/pkg_basic/caps.toml "$PKG_N/caps.toml"
cp tests/spec/pkg_basic/package.toml "$PKG_N/package.toml"
cp tests/spec/pkg_basic/pure.gcpatch "$PKG_N/pure.gcpatch"

native --selfhost-only --selfhost-artifact "$ART" pack --pkg "$PKG_N/package.toml" >/dev/null
native --selfhost-only --selfhost-artifact "$ART" typecheck --pkg "$PKG_N/package.toml" >/dev/null
native --selfhost-only --selfhost-artifact "$ART" test --pkg "$PKG_N/package.toml" >/dev/null
native --selfhost-only --selfhost-artifact "$ART" apply-patch "$PKG_N/pure.gcpatch" --pkg "$PKG_N/package.toml" >/dev/null

# strict selfhost smoke (WASI CLI native-host binary)
wasi_native --selfhost-only --selfhost-artifact "$ART" fmt "$TMP_DIR/mod.gc" >/dev/null
wasi_native --selfhost-only --selfhost-artifact "$ART" eval "$TMP_DIR/mod.gc" >/dev/null

PKG_W="$TMP_DIR/pkg_wasi"
mkdir -p "$PKG_W"
cp tests/spec/pkg_basic/basic.gc "$PKG_W/basic.gc"
cp tests/spec/pkg_basic/caps.toml "$PKG_W/caps.toml"
cp tests/spec/pkg_basic/package.toml "$PKG_W/package.toml"
wasi_native --selfhost-only --selfhost-artifact "$ART" pack --pkg "$PKG_W/package.toml" >/dev/null
wasi_native --selfhost-only --selfhost-artifact "$ART" test --pkg "$PKG_W/package.toml" >/dev/null

echo "selfhost-strict smoke: ok"
