#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"
export GENESIS_ALLOW_RUST_ENGINE=1

cargo build -p gc_cli -p gc_wasi_cli >/dev/null

GEN="$ROOT_DIR/target/debug/genesis"
GWASI="$ROOT_DIR/target/debug/genesis_wasi"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

fail() {
  echo "host-abi-conformance: $*" >&2
  exit 1
}

same() {
  local label="$1"
  local left="$2"
  local right="$3"
  [[ "$left" == "$right" ]] || fail "${label} mismatch"
}

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

# Core canonicalization/hash parity.
cp "$ROOT_DIR/tests/spec/coreform/app_sugar.in.gc" "$TMP_DIR/basic.native.gc"
cp "$ROOT_DIR/tests/spec/coreform/app_sugar.in.gc" "$TMP_DIR/basic.wasi.gc"
"$GEN" fmt "$TMP_DIR/basic.native.gc" >/dev/null
"$GWASI" fmt "$TMP_DIR/basic.wasi.gc" >/dev/null
diff -u "$TMP_DIR/basic.native.gc" "$TMP_DIR/basic.wasi.gc" >/dev/null || fail "fmt native vs wasi mismatch"

core_rust_hash_native="$("$GEN" vcs hash --in "$TMP_DIR/basic.native.gc" --engine rust | tr -d '\n')"
core_rust_hash_wasi="$("$GWASI" vcs hash --in "$TMP_DIR/basic.wasi.gc" --engine rust | tr -d '\n')"
same "vcs-hash/rust" "$core_rust_hash_native" "$core_rust_hash_wasi"

core_self_hash_native="$("$GEN" --selfhost-only --selfhost-artifact "$ART" vcs hash --in "$TMP_DIR/basic.native.gc" --engine selfhost | tr -d '\n')"
core_self_hash_wasi="$("$GWASI" --selfhost-only --selfhost-artifact "$ART" vcs hash --in "$TMP_DIR/basic.wasi.gc" --engine selfhost | tr -d '\n')"
same "vcs-hash/selfhost" "$core_self_hash_native" "$core_self_hash_wasi"
same "vcs-hash/cross-engine/native" "$core_rust_hash_native" "$core_self_hash_native"

# Pure eval parity.
cat >"$TMP_DIR/eval.gc" <<'GC'
(def abi::x (prim int/add 40 2))
abi::x
GC
eval_rust_native="$("$GEN" eval "$TMP_DIR/eval.gc" --engine rust | tr -d '\n')"
eval_rust_wasi="$("$GWASI" eval "$TMP_DIR/eval.gc" --engine rust | tr -d '\n')"
same "eval/rust" "$eval_rust_native" "$eval_rust_wasi"

eval_self_native="$("$GEN" --selfhost-only --selfhost-artifact "$ART" eval "$TMP_DIR/eval.gc" --engine selfhost | tr -d '\n')"
eval_self_wasi="$("$GWASI" --selfhost-only --selfhost-artifact "$ART" eval "$TMP_DIR/eval.gc" --engine selfhost | tr -d '\n')"
same "eval/selfhost" "$eval_self_native" "$eval_self_wasi"
same "eval/cross-engine/native" "$eval_rust_native" "$eval_self_native"

# run/replay parity.
cat >"$TMP_DIR/run.gc" <<'GC'
(def abi::prog (core/effect::pure 7))
abi::prog
GC
cat >"$TMP_DIR/caps.toml" <<'TOML'
allow = []
TOML

run_rust_native="$("$GEN" run "$TMP_DIR/run.gc" --engine rust --caps "$TMP_DIR/caps.toml" --log "$TMP_DIR/native.rust.gclog" | tr -d '\n')"
run_rust_wasi="$("$GWASI" run "$TMP_DIR/run.gc" --engine rust --caps "$TMP_DIR/caps.toml" --log "$TMP_DIR/wasi.rust.gclog" | tr -d '\n')"
same "run/rust" "$run_rust_native" "$run_rust_wasi"

run_self_native="$("$GEN" --selfhost-only --selfhost-artifact "$ART" run "$TMP_DIR/run.gc" --engine selfhost --caps "$TMP_DIR/caps.toml" --log "$TMP_DIR/native.self.gclog" | tr -d '\n')"
run_self_wasi="$("$GWASI" --selfhost-only --selfhost-artifact "$ART" run "$TMP_DIR/run.gc" --engine selfhost --caps "$TMP_DIR/caps.toml" --log "$TMP_DIR/wasi.self.gclog" | tr -d '\n')"
same "run/selfhost" "$run_self_native" "$run_self_wasi"
same "run/cross-engine/native" "$run_rust_native" "$run_self_native"

replay_rust_native="$("$GEN" replay "$TMP_DIR/run.gc" --engine rust --log "$TMP_DIR/native.rust.gclog" | tr -d '\n')"
replay_rust_wasi="$("$GWASI" replay "$TMP_DIR/run.gc" --engine rust --log "$TMP_DIR/wasi.rust.gclog" | tr -d '\n')"
same "replay/rust" "$replay_rust_native" "$replay_rust_wasi"

replay_self_native="$("$GEN" --selfhost-only --selfhost-artifact "$ART" replay "$TMP_DIR/run.gc" --engine selfhost --log "$TMP_DIR/native.self.gclog" | tr -d '\n')"
replay_self_wasi="$("$GWASI" --selfhost-only --selfhost-artifact "$ART" replay "$TMP_DIR/run.gc" --engine selfhost --log "$TMP_DIR/wasi.self.gclog" | tr -d '\n')"
same "replay/selfhost" "$replay_self_native" "$replay_self_wasi"
same "replay/cross-engine/native" "$replay_rust_native" "$replay_self_native"

# Package workflow parity for representative fixture.
for frontend in rust selfhost; do
  n_pkg="$TMP_DIR/pkg.native.$frontend"
  w_pkg="$TMP_DIR/pkg.wasi.$frontend"
  cp -R "$ROOT_DIR/tests/spec/pkg_basic" "$n_pkg"
  cp -R "$ROOT_DIR/tests/spec/pkg_basic" "$w_pkg"
  if [[ "$frontend" == "selfhost" ]]; then
    type_native="$("$GEN" --selfhost-only --selfhost-artifact "$ART" --coreform-frontend "$frontend" typecheck --pkg "$n_pkg/package.toml" | tr -d '\n')"
    type_wasi="$("$GWASI" --selfhost-only --selfhost-artifact "$ART" --coreform-frontend "$frontend" typecheck --pkg "$w_pkg/package.toml" | tr -d '\n')"
    pack_native="$("$GEN" --selfhost-only --selfhost-artifact "$ART" --coreform-frontend "$frontend" pack --pkg "$n_pkg/package.toml" | tr -d '\n')"
    pack_wasi="$("$GWASI" --selfhost-only --selfhost-artifact "$ART" --coreform-frontend "$frontend" pack --pkg "$w_pkg/package.toml" | tr -d '\n')"
    test_native="$("$GEN" --selfhost-only --selfhost-artifact "$ART" --coreform-frontend "$frontend" test --pkg "$n_pkg/package.toml" | tr -d '\n')"
    test_wasi="$("$GWASI" --selfhost-only --selfhost-artifact "$ART" --coreform-frontend "$frontend" test --pkg "$w_pkg/package.toml" | tr -d '\n')"
    patch_native="$("$GEN" --selfhost-only --selfhost-artifact "$ART" --coreform-frontend "$frontend" apply-patch "$n_pkg/pure.gcpatch" --pkg "$n_pkg/package.toml" | tr -d '\n')"
    patch_wasi="$("$GWASI" --selfhost-only --selfhost-artifact "$ART" --coreform-frontend "$frontend" apply-patch "$w_pkg/pure.gcpatch" --pkg "$w_pkg/package.toml" | tr -d '\n')"
  else
    type_native="$("$GEN" --coreform-frontend "$frontend" typecheck --pkg "$n_pkg/package.toml" | tr -d '\n')"
    type_wasi="$("$GWASI" --coreform-frontend "$frontend" typecheck --pkg "$w_pkg/package.toml" | tr -d '\n')"
    pack_native="$("$GEN" --coreform-frontend "$frontend" pack --pkg "$n_pkg/package.toml" | tr -d '\n')"
    pack_wasi="$("$GWASI" --coreform-frontend "$frontend" pack --pkg "$w_pkg/package.toml" | tr -d '\n')"
    test_native="$("$GEN" --coreform-frontend "$frontend" test --pkg "$n_pkg/package.toml" | tr -d '\n')"
    test_wasi="$("$GWASI" --coreform-frontend "$frontend" test --pkg "$w_pkg/package.toml" | tr -d '\n')"
    patch_native="$("$GEN" --coreform-frontend "$frontend" apply-patch "$n_pkg/pure.gcpatch" --pkg "$n_pkg/package.toml" | tr -d '\n')"
    patch_wasi="$("$GWASI" --coreform-frontend "$frontend" apply-patch "$w_pkg/pure.gcpatch" --pkg "$w_pkg/package.toml" | tr -d '\n')"
  fi
  same "typecheck/$frontend" "$type_native" "$type_wasi"
  same "pack/$frontend" "$pack_native" "$pack_wasi"
  same "test/$frontend" "$test_native" "$test_wasi"
  same "apply-patch/$frontend" "$patch_native" "$patch_wasi"
done

echo "host-abi-conformance: ok"
