#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"
export GENESIS_ALLOW_RUST_ENGINE=1 # rust-engine compatibility mode for parity/historical comparisons only

cargo build -p gc_cli -p gc_wasi_cli >/dev/null

GEN="$ROOT_DIR/target/debug/genesis"
GWASI="$ROOT_DIR/target/debug/genesis_wasi"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

ART="$TMP_DIR/selfhost_toolchain.gc"
REPO_ART="$ROOT_DIR/selfhost/toolchain.gc"
NEED_REBUILD=0
if [[ "${GENESIS_REBUILD_SELFHOST_ARTIFACT:-0}" == "1" ]]; then
  NEED_REBUILD=1
elif [[ ! -f "$REPO_ART" ]]; then
  NEED_REBUILD=1
elif [[ -n "$(find "$ROOT_DIR/selfhost" -maxdepth 1 -name '*.gc' -newer "$REPO_ART" -print -quit)" ]]; then
  NEED_REBUILD=1
fi
if [[ "$NEED_REBUILD" == "1" ]]; then
  "$GEN" selfhost-artifact --out "$ART" >/dev/null
else
  cp "$REPO_ART" "$ART"
fi

fail() {
  echo "selfhost-strict-golden: $*" >&2
  exit 1
}

check_typecheck_parity() {
  local pkg_toml="$1"
  local name="$2"
  local rust_out self_out rust_code self_code
  rust_out="$TMP_DIR/typecheck.${name}.rust.out"
  self_out="$TMP_DIR/typecheck.${name}.selfhost.out"

  set +e
  "$GEN" --coreform-frontend rust typecheck --pkg "$pkg_toml" >"$rust_out" 2>&1
  rust_code=$?
  "$GEN" --selfhost-only --selfhost-artifact "$ART" --coreform-frontend selfhost typecheck --pkg "$pkg_toml" >"$self_out" 2>&1
  self_code=$?
  set -e

  [[ "$rust_code" == "$self_code" ]] || fail "native strict typecheck exit mismatch for fixture ${name} (rust=${rust_code} selfhost=${self_code})"
  diff -u "$rust_out" "$self_out" >/dev/null || fail "native strict typecheck output mismatch for fixture ${name}"
}

check_coreform_fixture() {
  local in_file="$1"
  local base expected staged rust_opt self_opt rust_h self_h wasi_h wasi_rust_h wasi_rust_opt wasi_self_opt
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

  # WASI strict parity checks for canonicalized fixture outputs.
  cp "$in_file" "$TMP_DIR/${base}.wasi.gc"
  "$GWASI" --selfhost-only --selfhost-artifact "$ART" fmt "$TMP_DIR/${base}.wasi.gc" >/dev/null
  diff -u "$expected" "$TMP_DIR/${base}.wasi.gc" >/dev/null || fail "WASI fmt mismatch for ${base}.in.gc"
  wasi_rust_h="$("$GWASI" vcs hash --in "$expected" --engine rust | tr -d '\n')"
  wasi_h="$("$GWASI" --selfhost-only --selfhost-artifact "$ART" vcs hash --in "$expected" --engine selfhost | tr -d '\n')"
  [[ "$rust_h" == "$wasi_rust_h" ]] || fail "WASI rust vcs hash mismatch for ${base}.out.gc"
  [[ "$rust_h" == "$wasi_h" ]] || fail "WASI strict vcs hash mismatch for ${base}.out.gc"
  wasi_rust_opt="$TMP_DIR/${base}.opt.wasi.rust.gc"
  wasi_self_opt="$TMP_DIR/${base}.opt.wasi.self.gc"
  "$GWASI" optimize "$expected" --engine rust --out "$wasi_rust_opt" >/dev/null
  "$GWASI" --selfhost-only --selfhost-artifact "$ART" optimize "$expected" --out "$wasi_self_opt" >/dev/null
  diff -u "$wasi_rust_opt" "$wasi_self_opt" >/dev/null || fail "WASI strict optimize mismatch for ${base}.out.gc"
  diff -u "$rust_opt" "$wasi_rust_opt" >/dev/null || fail "WASI rust optimize mismatch for ${base}.out.gc"
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
wasi_rust_eval="$("$GWASI" eval "$TMP_DIR/eval_pure.gc" --engine rust | tr -d '\n')"
[[ "$rust_eval" == "$self_eval" ]] || fail "native strict eval mismatch on pure parity module"
wasi_eval="$("$GWASI" --selfhost-only --selfhost-artifact "$ART" eval "$TMP_DIR/eval_pure.gc" | tr -d '\n')"
[[ "$wasi_rust_eval" == "$wasi_eval" ]] || fail "WASI strict eval mismatch vs WASI rust baseline on pure parity module"
[[ "$rust_eval" == "$wasi_rust_eval" ]] || fail "WASI rust eval mismatch on pure parity module"
[[ "$rust_eval" == "$wasi_eval" ]] || fail "WASI strict eval mismatch on pure parity module"

# Dedicated run/replay parity module (native rust baseline vs native/WASI strict selfhost).
cat >"$TMP_DIR/run_pure.gc" <<'GC'
(def prog (core/effect::pure 99))
prog
GC
cat >"$TMP_DIR/run_caps.toml" <<'TOML'
allow = []
TOML

rust_run="$("$GEN" run "$TMP_DIR/run_pure.gc" --engine rust --caps "$TMP_DIR/run_caps.toml" --log "$TMP_DIR/run_pure.rust.gclog" | tr -d '\n')"
self_run="$("$GEN" --selfhost-only --selfhost-artifact "$ART" run "$TMP_DIR/run_pure.gc" --engine selfhost --caps "$TMP_DIR/run_caps.toml" --log "$TMP_DIR/run_pure.self.gclog" | tr -d '\n')"
wasi_rust_run="$("$GWASI" run "$TMP_DIR/run_pure.gc" --engine rust --caps "$TMP_DIR/run_caps.toml" --log "$TMP_DIR/run_pure.wasi.rust.gclog" | tr -d '\n')"
wasi_run="$("$GWASI" --selfhost-only --selfhost-artifact "$ART" run "$TMP_DIR/run_pure.gc" --engine selfhost --caps "$TMP_DIR/run_caps.toml" --log "$TMP_DIR/run_pure.wasi.self.gclog" | tr -d '\n')"
[[ "$rust_run" == "$self_run" ]] || fail "native strict run mismatch on pure parity module"
[[ "$wasi_rust_run" == "$wasi_run" ]] || fail "WASI strict run mismatch vs WASI rust baseline on pure parity module"
[[ "$rust_run" == "$wasi_run" ]] || fail "WASI strict run mismatch on pure parity module"
diff -u "$TMP_DIR/run_pure.rust.gclog" "$TMP_DIR/run_pure.self.gclog" >/dev/null || fail "native strict run log mismatch on pure parity module"
diff -u "$TMP_DIR/run_pure.wasi.rust.gclog" "$TMP_DIR/run_pure.wasi.self.gclog" >/dev/null || fail "WASI strict run log mismatch on pure parity module"

rust_replay="$("$GEN" replay "$TMP_DIR/run_pure.gc" --engine rust --log "$TMP_DIR/run_pure.rust.gclog" | tr -d '\n')"
self_replay="$("$GEN" --selfhost-only --selfhost-artifact "$ART" replay "$TMP_DIR/run_pure.gc" --engine selfhost --log "$TMP_DIR/run_pure.self.gclog" | tr -d '\n')"
wasi_rust_replay="$("$GWASI" replay "$TMP_DIR/run_pure.gc" --engine rust --log "$TMP_DIR/run_pure.wasi.rust.gclog" | tr -d '\n')"
wasi_replay="$("$GWASI" --selfhost-only --selfhost-artifact "$ART" replay "$TMP_DIR/run_pure.gc" --engine selfhost --log "$TMP_DIR/run_pure.wasi.self.gclog" | tr -d '\n')"
[[ "$rust_replay" == "$self_replay" ]] || fail "native strict replay mismatch on pure parity module"
[[ "$wasi_rust_replay" == "$wasi_replay" ]] || fail "WASI strict replay mismatch vs WASI rust baseline on pure parity module"
[[ "$rust_replay" == "$wasi_replay" ]] || fail "WASI strict replay mismatch on pure parity module"

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
  check_typecheck_parity "$pkg_toml" "$name"

  if [[ "$name" == pkg_fail_* ]]; then
    if "$GEN" --coreform-frontend rust test --pkg "$pkg_toml" >/dev/null 2>&1; then
      fail "expected rust test failure for fixture ${name}"
    fi
    if "$GEN" --selfhost-only --selfhost-artifact "$ART" --coreform-frontend selfhost test --pkg "$pkg_toml" >/dev/null 2>&1; then
      fail "expected strict selfhost test failure for fixture ${name}"
    fi
  else
    rust_pack="$("$GEN" --coreform-frontend rust pack --pkg "$pkg_toml" | tr -d '\n')"
    self_pack="$("$GEN" --selfhost-only --selfhost-artifact "$ART" --coreform-frontend selfhost pack --pkg "$pkg_toml" | tr -d '\n')"
    [[ "$rust_pack" == "$self_pack" ]] || fail "native strict pack mismatch for fixture ${name}"

    rust_test="$("$GEN" --coreform-frontend rust test --pkg "$pkg_toml" | tr -d '\n')"
    self_test="$("$GEN" --selfhost-only --selfhost-artifact "$ART" --coreform-frontend selfhost test --pkg "$pkg_toml" | tr -d '\n')"
    [[ "$rust_test" == "$self_test" ]] || fail "native strict test mismatch for fixture ${name}"
  fi
done

# Ensure strict selfhost package paths in WASI remain healthy on canonical baseline fixture.
PKG_W="$TMP_DIR/pkg_wasi"
cp -R "$ROOT_DIR/tests/spec/pkg_basic" "$PKG_W"
wasi_rust_pack="$("$GWASI" --coreform-frontend rust pack --pkg "$PKG_W/package.toml" | tr -d '\n')"
wasi_self_pack="$("$GWASI" --selfhost-only --selfhost-artifact "$ART" --coreform-frontend selfhost pack --pkg "$PKG_W/package.toml" | tr -d '\n')"
[[ "$wasi_rust_pack" == "$wasi_self_pack" ]] || fail "WASI strict pack mismatch for pkg_basic fixture"

wasi_rust_test="$("$GWASI" --coreform-frontend rust test --pkg "$PKG_W/package.toml" | tr -d '\n')"
wasi_self_test="$("$GWASI" --selfhost-only --selfhost-artifact "$ART" --coreform-frontend selfhost test --pkg "$PKG_W/package.toml" | tr -d '\n')"
[[ "$wasi_rust_test" == "$wasi_self_test" ]] || fail "WASI strict test mismatch for pkg_basic fixture"
wasi_rust_typecheck="$("$GWASI" --coreform-frontend rust typecheck --pkg "$PKG_W/package.toml" | tr -d '\n')"
wasi_self_typecheck="$("$GWASI" --selfhost-only --selfhost-artifact "$ART" --coreform-frontend selfhost typecheck --pkg "$PKG_W/package.toml" | tr -d '\n')"
[[ "$wasi_rust_typecheck" == "$wasi_self_typecheck" ]] || fail "WASI strict typecheck mismatch for pkg_basic fixture"

# Strict apply-patch + dashboard on native and WASI paths.
PKG_N_R="$TMP_DIR/pkg_native_rust"
PKG_N_S="$TMP_DIR/pkg_native_selfhost"
cp -R "$ROOT_DIR/tests/spec/pkg_basic" "$PKG_N_R"
cp -R "$ROOT_DIR/tests/spec/pkg_basic" "$PKG_N_S"
rust_patch="$("$GEN" --coreform-frontend rust apply-patch "$PKG_N_R/pure.gcpatch" --pkg "$PKG_N_R/package.toml" | tr -d '\n')"
self_patch="$("$GEN" --selfhost-only --selfhost-artifact "$ART" --coreform-frontend selfhost apply-patch "$PKG_N_S/pure.gcpatch" --pkg "$PKG_N_S/package.toml" | tr -d '\n')"
[[ "$rust_patch" == "$self_patch" ]] || fail "native strict apply-patch mismatch for pkg_basic fixture"

PKG_W_R="$TMP_DIR/pkg_wasi_rust"
PKG_W_S="$TMP_DIR/pkg_wasi_selfhost"
cp -R "$ROOT_DIR/tests/spec/pkg_basic" "$PKG_W_R"
cp -R "$ROOT_DIR/tests/spec/pkg_basic" "$PKG_W_S"
wasi_rust_patch="$("$GWASI" --coreform-frontend rust apply-patch "$PKG_W_R/pure.gcpatch" --pkg "$PKG_W_R/package.toml" | tr -d '\n')"
wasi_self_patch="$("$GWASI" --selfhost-only --selfhost-artifact "$ART" --coreform-frontend selfhost apply-patch "$PKG_W_S/pure.gcpatch" --pkg "$PKG_W_S/package.toml" | tr -d '\n')"
[[ "$wasi_rust_patch" == "$wasi_self_patch" ]] || fail "WASI strict apply-patch mismatch for pkg_basic fixture"
[[ "$rust_patch" == "$wasi_rust_patch" ]] || fail "WASI rust apply-patch mismatch for pkg_basic fixture"

"$GEN" --selfhost-only --selfhost-artifact "$ART" selfhost-dashboard --store "$TMP_DIR/store" --markdown "$TMP_DIR/SELFHOST_CUTOVER.md" >/dev/null
"$GWASI" --selfhost-only --selfhost-artifact "$ART" selfhost-dashboard --store "$TMP_DIR/wasi.store" --markdown "$TMP_DIR/WASI_SELFHOST_CUTOVER.md" >/dev/null
grep -q '\`policy/\*\`' "$TMP_DIR/SELFHOST_CUTOVER.md" || fail "native selfhost dashboard markdown missing policy/* row"
grep -q '\`policy/\*\`' "$TMP_DIR/WASI_SELFHOST_CUTOVER.md" || fail "WASI selfhost dashboard markdown missing policy/* row"

echo "selfhost-strict-golden: ok"
