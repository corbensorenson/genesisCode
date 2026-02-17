#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

cargo build -p gc_cli -p gc_wasi_cli >/dev/null

GEN="$ROOT_DIR/target/debug/genesis"
GWASI="$ROOT_DIR/target/debug/genesis_wasi"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

ART="$TMP_DIR/selfhost_toolchain.gc"
"$GEN" selfhost-artifact --out "$ART" >/dev/null

fail() {
  echo "selfhost-strict-golden: $*" >&2
  exit 1
}

check_coreform_fixture() {
  local in_file="$1"
  local base expected staged rust_opt self_opt rust_h self_h wasi_h
  base="$(basename "$in_file" .in.gc)"
  expected="$ROOT_DIR/tests/spec/coreform/${base}.out.gc"
  staged="$TMP_DIR/${base}.gc"
  cp "$in_file" "$staged"

  "$GEN" --selfhost-only --selfhost-artifact "$ART" fmt "$staged" >/dev/null
  diff -u "$expected" "$staged" >/dev/null || fail "fmt mismatch for ${base}.in.gc"
  "$GEN" --selfhost-only --selfhost-artifact "$ART" fmt "$staged" --check >/dev/null

  rust_h="$("$GEN" vcs hash --in "$expected" --engine rust | tr -d '\n')"
  self_h="$("$GEN" --selfhost-only --selfhost-artifact "$ART" vcs hash --in "$expected" --engine selfhost | tr -d '\n')"
  [[ "$rust_h" == "$self_h" ]] || fail "native strict vcs hash mismatch for ${base}.out.gc"

  rust_opt="$TMP_DIR/${base}.opt.rust.gc"
  self_opt="$TMP_DIR/${base}.opt.self.gc"
  "$GEN" optimize "$expected" --out "$rust_opt" >/dev/null
  "$GEN" --selfhost-only --selfhost-artifact "$ART" optimize "$expected" --out "$self_opt" >/dev/null
  diff -u "$rust_opt" "$self_opt" >/dev/null || fail "optimize mismatch for ${base}.out.gc"

  # WASI bootstrap currently routes fmt/eval/test/pack in strict mode.
  cp "$in_file" "$TMP_DIR/${base}.wasi.gc"
  "$GWASI" --selfhost-only --selfhost-artifact "$ART" fmt "$TMP_DIR/${base}.wasi.gc" >/dev/null
  diff -u "$expected" "$TMP_DIR/${base}.wasi.gc" >/dev/null || fail "WASI fmt mismatch for ${base}.in.gc"
  wasi_h="$("$GWASI" --selfhost-only --selfhost-artifact "$ART" vcs hash --in "$expected" --engine selfhost | tr -d '\n')"
  [[ "$rust_h" == "$wasi_h" ]] || fail "WASI strict vcs hash mismatch for ${base}.out.gc"
}

for in_file in "$ROOT_DIR"/tests/spec/coreform/*.in.gc; do
  check_coreform_fixture "$in_file"
done

# Dedicated pure eval parity module (native + WASI strict).
cat >"$TMP_DIR/eval_pure.gc" <<'GC'
(def m::x (prim int/add 1 2))
m::x
GC
rust_eval="$("$GEN" eval "$TMP_DIR/eval_pure.gc" | tr -d '\n')"
self_eval="$("$GEN" --selfhost-only --selfhost-artifact "$ART" eval "$TMP_DIR/eval_pure.gc" | tr -d '\n')"
[[ "$rust_eval" == "$self_eval" ]] || fail "native strict eval mismatch on pure parity module"
wasi_eval="$("$GWASI" --selfhost-only --selfhost-artifact "$ART" eval "$TMP_DIR/eval_pure.gc" | tr -d '\n')"
[[ "$rust_eval" == "$wasi_eval" ]] || fail "WASI strict eval mismatch on pure parity module"

# Package golden sweep (selfhost strict) over every package fixture.
PKGS_TMP="$TMP_DIR/pkgs"
mkdir -p "$PKGS_TMP"
for src_dir in "$ROOT_DIR"/tests/spec/pkg_*; do
  [[ -d "$src_dir" ]] || continue
  [[ -f "$src_dir/package.toml" ]] || continue
  name="$(basename "$src_dir")"
  dst_dir="$PKGS_TMP/$name"
  cp -R "$src_dir" "$dst_dir"
  pkg_toml="$dst_dir/package.toml"

  if [[ "$name" == pkg_fail_* ]]; then
    if "$GEN" --selfhost-only --selfhost-artifact "$ART" test --pkg "$pkg_toml" >/dev/null 2>&1; then
      fail "expected strict selfhost test failure for fixture ${name}"
    fi
  else
    "$GEN" --selfhost-only --selfhost-artifact "$ART" test --pkg "$pkg_toml" >/dev/null
  fi
done

# Ensure strict selfhost package paths in WASI remain healthy on canonical baseline fixture.
PKG_W="$TMP_DIR/pkg_wasi"
cp -R "$ROOT_DIR/tests/spec/pkg_basic" "$PKG_W"
"$GWASI" --selfhost-only --selfhost-artifact "$ART" pack --pkg "$PKG_W/package.toml" >/dev/null
"$GWASI" --selfhost-only --selfhost-artifact "$ART" test --pkg "$PKG_W/package.toml" >/dev/null

# Strict apply-patch + dashboard on native path.
PKG_N="$TMP_DIR/pkg_native"
cp -R "$ROOT_DIR/tests/spec/pkg_basic" "$PKG_N"
"$GEN" --selfhost-only --selfhost-artifact "$ART" apply-patch "$PKG_N/pure.gcpatch" --pkg "$PKG_N/package.toml" >/dev/null
"$GEN" --selfhost-only --selfhost-artifact "$ART" selfhost-dashboard --store "$TMP_DIR/store" --markdown "$TMP_DIR/SELFHOST_CUTOVER.md" >/dev/null

echo "selfhost-strict-golden: ok"
