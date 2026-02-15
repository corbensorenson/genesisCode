#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if ! command -v wasmtime >/dev/null 2>&1; then
  echo "wasmtime is required for wasi_smoke.sh" >&2
  exit 1
fi

WASM_BIN="${1:-}"
if [[ -z "${WASM_BIN}" ]]; then
  WASM_BIN="$(bash scripts/build_wasi.sh)"
fi

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

cp tests/spec/coreform/app_sugar.in.gc "$TMP_DIR/in.gc"
cp tests/spec/coreform/app_sugar.out.gc "$TMP_DIR/out.gc"

# fmt --check must fail on non-canonical input.
set +e
wasmtime --dir "$TMP_DIR" "$WASM_BIN" fmt "$TMP_DIR/in.gc" --check >/dev/null 2>&1
CODE=$?
set -e
if [[ "$CODE" -ne 11 ]]; then
  echo "expected fmt --check to exit 11, got $CODE" >&2
  exit 1
fi

# fmt should rewrite to canonical output.
wasmtime --dir "$TMP_DIR" "$WASM_BIN" fmt "$TMP_DIR/in.gc" >/dev/null
diff -u "$TMP_DIR/out.gc" "$TMP_DIR/in.gc" >/dev/null

# vcs hash should match native genesis for the same file.
NATIVE_HASH="$(cargo run -p gc_cli --quiet -- vcs hash --in tests/spec/coreform/map_order.in.gc | tr -d '\n')"
WASI_HASH="$(wasmtime --dir . "$WASM_BIN" vcs hash --in tests/spec/coreform/map_order.in.gc | tr -d '\n')"
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
WASI_EVAL="$(wasmtime --dir "$TMP_DIR" "$WASM_BIN" eval "$TMP_DIR/eval.gc" | tr -d '\n')"
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
WASI_RUN="$(wasmtime --dir "$TMP_DIR" "$WASM_BIN" run "$TMP_DIR/run.gc" --caps "$TMP_DIR/caps.toml" --log "$TMP_DIR/wasi.gclog" | tr -d '\n')"
if [[ "$NATIVE_RUN" != "$WASI_RUN" ]]; then
  echo "run mismatch native=$NATIVE_RUN wasi=$WASI_RUN" >&2
  exit 1
fi

NATIVE_REPLAY="$(cargo run -p gc_cli --quiet -- replay "$TMP_DIR/run.gc" --log "$TMP_DIR/native.gclog" | tr -d '\n')"
WASI_REPLAY="$(wasmtime --dir "$TMP_DIR" "$WASM_BIN" replay "$TMP_DIR/run.gc" --log "$TMP_DIR/wasi.gclog" | tr -d '\n')"
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
POLICY_H_WASI="$(wasmtime --dir "$TMP_DIR" "$WASM_BIN" store --caps "$TMP_DIR/caps_store_refs.toml" --log "$TMP_DIR/wasi-store-put.gclog" put --in "$TMP_DIR/policy.gc" | tr -d '\n')"
if [[ "$POLICY_H_NATIVE" != "$POLICY_H_WASI" ]]; then
  echo "store put hash mismatch native=$POLICY_H_NATIVE wasi=$POLICY_H_WASI" >&2
  exit 1
fi

NATIVE_POLICY_TERM="$(cargo run -p gc_cli --quiet -- store --caps "$TMP_DIR/caps_store_refs.toml" --log "$TMP_DIR/native-store-get.gclog" get "$POLICY_H_NATIVE" | tr -d '\n')"
WASI_POLICY_TERM="$(wasmtime --dir "$TMP_DIR" "$WASM_BIN" store --caps "$TMP_DIR/caps_store_refs.toml" --log "$TMP_DIR/wasi-store-get.gclog" get "$POLICY_H_NATIVE" | tr -d '\n')"
if [[ "$NATIVE_POLICY_TERM" != "$WASI_POLICY_TERM" ]]; then
  echo "store get mismatch native=$NATIVE_POLICY_TERM wasi=$WASI_POLICY_TERM" >&2
  exit 1
fi

cat >"$TMP_DIR/evidence.gc" <<'GC'
{:type :vcs/evidence :v 1 :kind :unit-tests :data nil}
GC
EVIDENCE_H_NATIVE="$(cargo run -p gc_cli --quiet -- store --caps "$TMP_DIR/caps_store_refs.toml" --log "$TMP_DIR/native-evidence-put.gclog" put --in "$TMP_DIR/evidence.gc" | tr -d '\n')"

Z64="$(python - <<'PY'\nprint('0'*64)\nPY\n)"
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
WASI_SET="$(wasmtime --dir "$TMP_DIR" "$WASM_BIN" refs --caps "$TMP_DIR/caps_store_refs.toml" --log "$TMP_DIR/wasi-refs-set.gclog" set "refs/heads/dev" "$COMMIT_H_NATIVE" --policy "$POLICY_H_NATIVE" | tr -d '\n')"
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
wasmtime --dir "$TMP_DIR" "$WASM_BIN" refs --caps "$TMP_DIR/caps_store_refs.toml" --log "$TMP_DIR/wasi-refs-cas.gclog" set "refs/heads/dev" "$COMMIT_H_NATIVE" --policy "$POLICY_H_NATIVE" --expected-old nil >/dev/null 2>&1
W_CODE=$?
set -e
if [[ "$N_CODE" -ne 20 || "$W_CODE" -ne 20 ]]; then
  echo "expected CAS conflict to exit 20 (native=$N_CODE wasi=$W_CODE)" >&2
  exit 1
fi

# delete via native, read via WASI.
cargo run -p gc_cli --quiet -- refs --caps "$TMP_DIR/caps_store_refs.toml" --log "$TMP_DIR/native-refs-del.gclog" delete "refs/heads/dev" --policy "$POLICY_H_NATIVE" >/dev/null
WASI_GET_AFTER_DEL="$(wasmtime --dir "$TMP_DIR" "$WASM_BIN" refs --caps "$TMP_DIR/caps_store_refs.toml" --log "$TMP_DIR/wasi-refs-get2.gclog" get "refs/heads/dev" | tr -d '\n')"
if [[ "$WASI_GET_AFTER_DEL" != "nil" ]]; then
  echo "refs get after delete mismatch wasi=$WASI_GET_AFTER_DEL expected=nil" >&2
  exit 1
fi

echo "ok"
