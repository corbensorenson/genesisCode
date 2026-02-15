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

echo "ok"
