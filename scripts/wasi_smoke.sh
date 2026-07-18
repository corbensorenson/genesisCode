#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "wasi-smoke" \
  root-wasi

if ! command -v wasmtime >/dev/null 2>&1; then
  echo "wasmtime is required for wasi_smoke.sh" >&2
  exit 1
fi

WASM_BIN="${1:-}"
if [[ -z "${WASM_BIN}" ]]; then
  rustup target add wasm32-wasip1 >/dev/null
  cargo build -p gc_wasi_cli --target wasm32-wasip1 --release >/dev/null
  WASM_BIN="$CARGO_TARGET_DIR/wasm32-wasip1/release/genesis_wasi.wasm"
fi

# Native comparison commands use the host scope; retain the resolved Wasm path.
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "wasi-smoke-native" \
  root-host

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT
SELFHOST_ARTIFACT="$TMP_DIR/selfhost-toolchain.gc"
cp selfhost/toolchain.gc "$SELFHOST_ARTIFACT"
WASI=(
  wasmtime --dir . --dir "$TMP_DIR" "$WASM_BIN"
  --selfhost-artifact "$SELFHOST_ARTIFACT"
)

cp tests/spec/coreform/app_sugar.in.gc "$TMP_DIR/in.gc"
cp tests/spec/coreform/app_sugar.out.gc "$TMP_DIR/out.gc"
cp tests/spec/coreform/app_sugar.in.gc "$TMP_DIR/in2.gc"

# fmt --check must fail on non-canonical input.
set +e
"${WASI[@]}" fmt "$TMP_DIR/in.gc" --check >/dev/null 2>&1
CODE=$?
set -e
if [[ "$CODE" -ne 11 ]]; then
  echo "expected fmt --check to exit 11, got $CODE" >&2
  exit 1
fi

# fmt --check must also fail on non-canonical input when using the self-host engine.
set +e
"${WASI[@]}" fmt "$TMP_DIR/in2.gc" --check --engine selfhost >/dev/null 2>&1
CODE=$?
set -e
if [[ "$CODE" -ne 11 ]]; then
  echo "expected fmt --check --engine selfhost to exit 11, got $CODE" >&2
  exit 1
fi

# fmt should rewrite to canonical output.
"${WASI[@]}" fmt "$TMP_DIR/in.gc" >/dev/null
diff -u "$TMP_DIR/out.gc" "$TMP_DIR/in.gc" >/dev/null

# fmt should rewrite to canonical output with the self-host engine as well.
"${WASI[@]}" fmt "$TMP_DIR/in2.gc" --engine selfhost >/dev/null
diff -u "$TMP_DIR/out.gc" "$TMP_DIR/in2.gc" >/dev/null

# vcs hash should match native genesis for the same file.
NATIVE_HASH="$(cargo run -p gc_cli --quiet -- vcs hash --in tests/spec/coreform/map_order.in.gc | tr -d '\n')"
WASI_HASH="$("${WASI[@]}" vcs hash --in tests/spec/coreform/map_order.in.gc | tr -d '\n')"
if [[ "$NATIVE_HASH" != "$WASI_HASH" ]]; then
  echo "hash mismatch native=$NATIVE_HASH wasi=$WASI_HASH" >&2
  exit 1
fi

# eval should match native genesis for a pure program.
cat >"$TMP_DIR/eval.gc" <<'GC'
(def m::x 1)
m::x
GC
NATIVE_EVAL="$(cargo run -p gc_cli --quiet -- eval "$TMP_DIR/eval.gc" | tr -d '\n')"
WASI_EVAL="$("${WASI[@]}" eval "$TMP_DIR/eval.gc" | tr -d '\n')"
if [[ "$NATIVE_EVAL" != "$WASI_EVAL" ]]; then
  echo "eval mismatch native=$NATIVE_EVAL wasi=$WASI_EVAL" >&2
  exit 1
fi

# run/replay should match native genesis for a deterministic filesystem read.
cat >"$TMP_DIR/data.txt" <<'TXT'
hello
TXT

cat >"$TMP_DIR/caps.toml" <<'TOML'
allow = ["io/fs::read"]

[op."io/fs::read"]
base_dir = "."
TOML

cat >"$TMP_DIR/run.gc" <<'GC'
(def prog
  (core/effect::perform
    'io/fs::read
    { :path "data.txt" }
    (fn (b) (core/effect::pure b))))
prog
GC

NATIVE_RUN="$(cargo run -p gc_cli --quiet -- run "$TMP_DIR/run.gc" --caps "$TMP_DIR/caps.toml" --log "$TMP_DIR/native.gclog" | tr -d '\n')"
WASI_RUN="$("${WASI[@]}" run "$TMP_DIR/run.gc" --caps "$TMP_DIR/caps.toml" --log "$TMP_DIR/wasi.gclog" | tr -d '\n')"
if [[ "$NATIVE_RUN" != "$WASI_RUN" ]]; then
  echo "run mismatch native=$NATIVE_RUN wasi=$WASI_RUN" >&2
  exit 1
fi

NATIVE_REPLAY="$(cargo run -p gc_cli --quiet -- replay "$TMP_DIR/run.gc" --log "$TMP_DIR/native.gclog" | tr -d '\n')"
WASI_REPLAY="$("${WASI[@]}" replay "$TMP_DIR/run.gc" --log "$TMP_DIR/wasi.gclog" | tr -d '\n')"
if [[ "$NATIVE_REPLAY" != "$WASI_REPLAY" ]]; then
  echo "replay mismatch native=$NATIVE_REPLAY wasi=$WASI_REPLAY" >&2
  exit 1
fi

# store/refs should match native for the same artifacts.
cat >"$TMP_DIR/caps_store_refs.toml" <<'TOML'
allow = [
  "core/store::put",
  "core/store::get",
  "core/store::has",
  "core/refs::get",
  "core/refs::list",
  "core/refs::set",
  "core/refs::delete"
]

[store]
dir = "./.genesis/store"

[refs]
path = "./.genesis/refs.gc"
TOML

cat >"$TMP_DIR/policy.gc" <<'GC'
{
  :type :vcs/policy
  :v 1
  :refs {:frozen-prefixes ["refs/frozen/"]}
  :classes {
    :dev  {:patterns ["refs/**/heads/*"] :exclude ["refs/**/heads/main"]
           :required-obligations ["core/obligation::unit-tests"]}
    :main {:patterns ["refs/**/heads/main"]
           :required-obligations ["core/obligation::unit-tests"]}
    :tags {:patterns ["refs/**/tags/*"]
           :required-obligations ["core/obligation::unit-tests"]
           :require-signatures false}
  }
}
GC

POLICY_H_NATIVE="$(cargo run -p gc_cli --quiet -- store --caps "$TMP_DIR/caps_store_refs.toml" --log "$TMP_DIR/native-store-put.gclog" put --in "$TMP_DIR/policy.gc" | tr -d '\n')"
POLICY_H_WASI="$("${WASI[@]}" store --caps "$TMP_DIR/caps_store_refs.toml" --log "$TMP_DIR/wasi-store-put.gclog" put --in "$TMP_DIR/policy.gc" | tr -d '\n')"
if [[ "$POLICY_H_NATIVE" != "$POLICY_H_WASI" ]]; then
  echo "store put hash mismatch native=$POLICY_H_NATIVE wasi=$POLICY_H_WASI" >&2
  exit 1
fi

NATIVE_POLICY_TERM="$(cargo run -p gc_cli --quiet -- store --caps "$TMP_DIR/caps_store_refs.toml" --log "$TMP_DIR/native-store-get.gclog" get "$POLICY_H_NATIVE" | tr -d '\n')"
WASI_POLICY_TERM="$("${WASI[@]}" store --caps "$TMP_DIR/caps_store_refs.toml" --log "$TMP_DIR/wasi-store-get.gclog" get "$POLICY_H_NATIVE" | tr -d '\n')"
if [[ "$NATIVE_POLICY_TERM" != "$WASI_POLICY_TERM" ]]; then
  echo "store get mismatch native=$NATIVE_POLICY_TERM wasi=$WASI_POLICY_TERM" >&2
  exit 1
fi

cat >"$TMP_DIR/evidence.gc" <<'GC'
{:type :vcs/evidence :v 1 :kind :unit-tests :data nil}
GC
EVIDENCE_H_NATIVE="$(cargo run -p gc_cli --quiet -- store --caps "$TMP_DIR/caps_store_refs.toml" --log "$TMP_DIR/native-evidence-put.gclog" put --in "$TMP_DIR/evidence.gc" | tr -d '\n')"

Z64="$(printf '%064d' 0)"
cat >"$TMP_DIR/commit.gc" <<GC
{
  :type :vcs/commit
  :v 1
  :parents []
  :base nil
  :patch "$Z64"
  :result "$Z64"
  :obligations ["core/obligation::unit-tests"]
  :evidence ["$EVIDENCE_H_NATIVE"]
  :attestations []
  :message "test commit"
}
GC
COMMIT_H_NATIVE="$(cargo run -p gc_cli --quiet -- store --caps "$TMP_DIR/caps_store_refs.toml" --log "$TMP_DIR/native-commit-put.gclog" put --in "$TMP_DIR/commit.gc" | tr -d '\n')"

# refs set via WASI, read via native.
WASI_SET="$("${WASI[@]}" refs --caps "$TMP_DIR/caps_store_refs.toml" --log "$TMP_DIR/wasi-refs-set.gclog" set "refs/heads/dev" "$COMMIT_H_NATIVE" --policy "$POLICY_H_NATIVE" | tr -d '\n')"
if [[ "$WASI_SET" != "$COMMIT_H_NATIVE" ]]; then
  echo "refs set output mismatch wasi=$WASI_SET expected=$COMMIT_H_NATIVE" >&2
  exit 1
fi

NATIVE_GET="$(cargo run -p gc_cli --quiet -- refs --caps "$TMP_DIR/caps_store_refs.toml" --log "$TMP_DIR/native-refs-get.gclog" get "refs/heads/dev" | tr -d '\n')"
if [[ "$NATIVE_GET" != "$COMMIT_H_NATIVE" ]]; then
  echo "refs get mismatch native=$NATIVE_GET expected=$COMMIT_H_NATIVE" >&2
  exit 1
fi

# CAS conflict should exit 20 on both.
set +e
cargo run -p gc_cli --quiet -- refs --caps "$TMP_DIR/caps_store_refs.toml" --log "$TMP_DIR/native-refs-cas.gclog" set "refs/heads/dev" "$COMMIT_H_NATIVE" --policy "$POLICY_H_NATIVE" --expected-old nil >/dev/null 2>&1
N_CODE=$?
"${WASI[@]}" refs --caps "$TMP_DIR/caps_store_refs.toml" --log "$TMP_DIR/wasi-refs-cas.gclog" set "refs/heads/dev" "$COMMIT_H_NATIVE" --policy "$POLICY_H_NATIVE" --expected-old nil >/dev/null 2>&1
W_CODE=$?
set -e
if [[ "$N_CODE" -ne 20 || "$W_CODE" -ne 20 ]]; then
  echo "expected CAS conflict to exit 20 (native=$N_CODE wasi=$W_CODE)" >&2
  exit 1
fi

# delete via native, read via WASI.
cargo run -p gc_cli --quiet -- refs --caps "$TMP_DIR/caps_store_refs.toml" --log "$TMP_DIR/native-refs-del.gclog" delete "refs/heads/dev" --policy "$POLICY_H_NATIVE" >/dev/null
WASI_GET_AFTER_DEL="$("${WASI[@]}" refs --caps "$TMP_DIR/caps_store_refs.toml" --log "$TMP_DIR/wasi-refs-get2.gclog" get "refs/heads/dev" | tr -d '\n')"
if [[ "$WASI_GET_AFTER_DEL" != "nil" ]]; then
  echo "refs get after delete mismatch wasi=$WASI_GET_AFTER_DEL expected=nil" >&2
  exit 1
fi

# pkg workflows (local-only) should match native genesis for lock/init/add/lock/install.
cat >"$TMP_DIR/caps_pkg.toml" <<'TOML'
allow = [
  "core/store::put",
  "core/store::get",
  "core/store::has",
  "core/refs::get",
  "core/refs::list",
  "core/refs::set",
  "core/refs::delete",
  "core/pkg-low::init",
  "core/pkg-low::add",
  "core/pkg-low::save-lock",
  "core/pkg-low::load-lock",
  "core/pkg-low::lock",
  "core/pkg-low::install",
  "core/pkg-low::verify",
  "core/pkg-low::list",
  "core/pkg-low::info"
]

[store]
dir = "./.genesis/store"

[refs]
path = "./.genesis/refs.gc"

[op."core/pkg-low::init"]
base_dir = "."

[op."core/pkg-low::add"]
base_dir = "."

[op."core/pkg-low::save-lock"]
base_dir = "."

[op."core/pkg-low::load-lock"]
base_dir = "."

[op."core/pkg-low::lock"]
base_dir = "."

[op."core/pkg-low::install"]
base_dir = "."

[op."core/pkg-low::verify"]
base_dir = "."

[op."core/pkg-low::list"]
base_dir = "."

[op."core/pkg-low::info"]
base_dir = "."
TOML

cat >"$TMP_DIR/snap.gc" <<'GC'
{
  :type :vcs/snapshot
  :v 1
  :kind :package
  :pkg/name "mini"
  :pkg/version "0.0.0"
  :modules []
  :obligations []
}
GC

SNAP_H_NATIVE="$(cargo run -p gc_cli --quiet -- store --caps "$TMP_DIR/caps_pkg.toml" --log "$TMP_DIR/native-snap-put.gclog" put --in "$TMP_DIR/snap.gc" | tr -d '\n')"
SNAP_H_WASI="$("${WASI[@]}" store --caps "$TMP_DIR/caps_pkg.toml" --log "$TMP_DIR/wasi-snap-put.gclog" put --in "$TMP_DIR/snap.gc" | tr -d '\n')"
if [[ "$SNAP_H_NATIVE" != "$SNAP_H_WASI" ]]; then
  echo "snapshot hash mismatch native=$SNAP_H_NATIVE wasi=$SNAP_H_WASI" >&2
  exit 1
fi

N_INIT="$(cargo run -p gc_cli --quiet -- pkg --caps "$TMP_DIR/caps_pkg.toml" --log "$TMP_DIR/native-pkg-init.gclog" init --workspace "wasi-smoke" --lock genesis.lock | tr -d '\n')"
W_INIT="$("${WASI[@]}" pkg --caps "$TMP_DIR/caps_pkg.toml" --log "$TMP_DIR/wasi-pkg-init.gclog" init --workspace "wasi-smoke" --lock genesis.lock | tr -d '\n')"
if [[ "$N_INIT" != "$W_INIT" ]]; then
  echo "pkg init mismatch native=$N_INIT wasi=$W_INIT" >&2
  exit 1
fi

SPEC="mini@snapshot:$SNAP_H_NATIVE"
N_ADD="$(cargo run -p gc_cli --quiet -- pkg --caps "$TMP_DIR/caps_pkg.toml" --log "$TMP_DIR/native-pkg-add.gclog" add "$SPEC" --lock genesis.lock --update-policy manual | tr -d '\n')"
W_ADD="$("${WASI[@]}" pkg --caps "$TMP_DIR/caps_pkg.toml" --log "$TMP_DIR/wasi-pkg-add.gclog" add "$SPEC" --lock genesis.lock --update-policy manual | tr -d '\n')"
if [[ "$N_ADD" != "$W_ADD" ]]; then
  echo "pkg add mismatch native=$N_ADD wasi=$W_ADD" >&2
  exit 1
fi

N_LOCK="$(cargo run -p gc_cli --quiet -- pkg --caps "$TMP_DIR/caps_pkg.toml" --log "$TMP_DIR/native-pkg-lock.gclog" lock --lock genesis.lock | tr -d '\n')"
W_LOCK="$("${WASI[@]}" pkg --caps "$TMP_DIR/caps_pkg.toml" --log "$TMP_DIR/wasi-pkg-lock.gclog" lock --lock genesis.lock | tr -d '\n')"
if [[ "$N_LOCK" != "$W_LOCK" ]]; then
  echo "pkg lock mismatch native=$N_LOCK wasi=$W_LOCK" >&2
  exit 1
fi

N_INSTALL="$(cargo run -p gc_cli --quiet -- pkg --caps "$TMP_DIR/caps_pkg.toml" --log "$TMP_DIR/native-pkg-install.gclog" install --lock genesis.lock --frozen | tr -d '\n')"
W_INSTALL="$("${WASI[@]}" pkg --caps "$TMP_DIR/caps_pkg.toml" --log "$TMP_DIR/wasi-pkg-install.gclog" install --lock genesis.lock --frozen | tr -d '\n')"
if [[ "$N_INSTALL" != "$W_INSTALL" ]]; then
  echo "pkg install mismatch native=$N_INSTALL wasi=$W_INSTALL" >&2
  exit 1
fi

# pack/test should work under WASI and match native for the same package fixture (no ambient nondeterminism).
cp -R tests/spec/pkg_basic "$TMP_DIR/pkg_basic"
N_PACK="$(cargo run -p gc_cli --quiet -- pack --pkg "$TMP_DIR/pkg_basic/package.toml" | tr -d '\n')"
W_PACK="$("${WASI[@]}" pack --pkg "$TMP_DIR/pkg_basic/package.toml" | tr -d '\n')"
if [[ "$N_PACK" != "$W_PACK" ]]; then
  echo "pack mismatch native=$N_PACK wasi=$W_PACK" >&2
  exit 1
fi

N_TEST="$(cargo run -p gc_cli --quiet -- test --pkg "$TMP_DIR/pkg_basic/package.toml" | tr -d '\n')"
W_TEST="$("${WASI[@]}" test --pkg "$TMP_DIR/pkg_basic/package.toml" | tr -d '\n')"
if [[ "$N_TEST" != "$W_TEST" ]]; then
  echo "test mismatch native=$N_TEST wasi=$W_TEST" >&2
  exit 1
fi

echo "ok"
