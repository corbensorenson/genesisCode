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

cat >"$TMP_DIR/run.gc" <<'GC'
(def prog (core/effect::pure 42))
prog
GC
cat >"$TMP_DIR/caps.toml" <<'TOML'
allow = []
TOML

native --selfhost-only --selfhost-artifact "$ART" run "$TMP_DIR/run.gc" --engine selfhost --caps "$TMP_DIR/caps.toml" --log "$TMP_DIR/native.strict.gclog" >/dev/null
native --selfhost-only --selfhost-artifact "$ART" replay "$TMP_DIR/run.gc" --engine selfhost --log "$TMP_DIR/native.strict.gclog" >/dev/null

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
native --selfhost-only --selfhost-artifact "$ART" selfhost-dashboard --store "$TMP_DIR/store" --markdown "$TMP_DIR/SELFHOST_CUTOVER.md" >/dev/null
native --selfhost-only --selfhost-artifact "$ART" vcs hash --in "$TMP_DIR/mod.gc" --engine selfhost >/dev/null

# strict selfhost smoke (WASI CLI native-host binary)
wasi_native --selfhost-only --selfhost-artifact "$ART" fmt "$TMP_DIR/mod.gc" >/dev/null
W_N_EVAL="$(wasi_native eval "$TMP_DIR/mod.gc" --engine rust | tr -d '\n')"
W_S_EVAL="$(wasi_native --selfhost-only --selfhost-artifact "$ART" eval "$TMP_DIR/mod.gc" | tr -d '\n')"
if [[ "$W_N_EVAL" != "$W_S_EVAL" ]]; then
  echo "WASI strict eval mismatch rust=$W_N_EVAL strict=$W_S_EVAL" >&2
  exit 1
fi
if [[ "$N_EVAL" != "$W_N_EVAL" ]]; then
  echo "WASI rust eval mismatch native=$N_EVAL wasi=$W_N_EVAL" >&2
  exit 1
fi
wasi_native --selfhost-only --selfhost-artifact "$ART" run "$TMP_DIR/run.gc" --engine selfhost --caps "$TMP_DIR/caps.toml" --log "$TMP_DIR/wasi.strict.gclog" >/dev/null
wasi_native --selfhost-only --selfhost-artifact "$ART" replay "$TMP_DIR/run.gc" --engine selfhost --log "$TMP_DIR/wasi.strict.gclog" >/dev/null
wasi_native --selfhost-only --selfhost-artifact "$ART" vcs hash --in "$TMP_DIR/mod.gc" --engine selfhost >/dev/null

PKG_W="$TMP_DIR/pkg_wasi"
mkdir -p "$PKG_W"
cp tests/spec/pkg_basic/basic.gc "$PKG_W/basic.gc"
cp tests/spec/pkg_basic/caps.toml "$PKG_W/caps.toml"
cp tests/spec/pkg_basic/package.toml "$PKG_W/package.toml"
wasi_native --selfhost-only --selfhost-artifact "$ART" pack --pkg "$PKG_W/package.toml" >/dev/null
wasi_native --selfhost-only --selfhost-artifact "$ART" test --pkg "$PKG_W/package.toml" >/dev/null

echo "selfhost-strict smoke: ok"
