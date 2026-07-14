#!/usr/bin/env bash
set -euo pipefail

# RUST_ENGINE_COMPAT_EXCEPTION:
# This guard intentionally invokes `--engine rust` / `--coreform-frontend rust`
# to prove release binaries reject compatibility-mode execution unconditionally.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"

DISK_MIN_FREE_KB="${GENESIS_RELEASE_GUARD_MIN_FREE_KB:-2097152}"
DISK_STRICT_MODE="${GENESIS_RELEASE_GUARD_DISK_STRICT_MODE:-1}"

bash scripts/check_disk_headroom.sh \
  --path "$ROOT_DIR" \
  --context "selfhost-release-profile-guard" \
  --min-kb "$DISK_MIN_FREE_KB" \
  --strict "$DISK_STRICT_MODE"
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "selfhost-release-profile-guard" \
  root-host

DEFAULT_RELEASE_DIR="$ROOT_DIR/target/release"
if [[ -n "${CARGO_TARGET_DIR:-}" ]]; then
  DEFAULT_RELEASE_DIR="$CARGO_TARGET_DIR/release"
fi
GEN="${GEN_REL:-$DEFAULT_RELEASE_DIR/genesis}"
GWASI="${GWASI_REL:-$DEFAULT_RELEASE_DIR/genesis_wasi}"

cargo build -p gc_cli --release >/dev/null
cargo build -p gc_wasi_cli --release >/dev/null

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

assert_release_binary() {
  local label="$1"
  local path="$2"
  [[ -f "$path" ]] || fail "${label} missing release artifact at ${path}"
  [[ -x "$path" ]] || fail "${label} artifact is not executable (${path})"

  if command -v file >/dev/null 2>&1; then
    local desc
    desc="$(file -b "$path" 2>/dev/null || true)"
    case "$desc" in
      *"Mach-O"*|*"ELF"*|*"PE32"*|*"executable"*)
        ;;
      *)
        fail "${label} artifact does not look like an executable binary (file='${desc}')"
        ;;
    esac
  fi
}

expect_release_rejected() {
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

expect_bootstrap_mode_parse_rejected() {
  local label="$1"
  shift
  set +e
  local out
  out="$("$@" 2>&1)"
  local code=$?
  set -e
  [[ "$code" == "2" ]] || fail "$label expected exit 2, got $code (out=$out)"
  [[ "$out" == *"invalid value 'embedded'"* && "$out" == *"selfhost-bootstrap"* ]] || {
    fail "$label missing parse-level embedded rejection (out=$out)"
  }
  [[ "$out" == *"artifact-only"* ]] || {
    fail "$label missing artifact-only expectation hint (out=$out)"
  }
}

assert_release_binary "native release" "$GEN"
assert_release_binary "wasi release" "$GWASI"

# Native release binary: rust compatibility must remain disabled regardless of env flag.
expect_release_rejected "native release fmt --engine rust" \
  "$GEN" fmt "$TMP_DIR/mod.gc" --engine rust
expect_release_rejected "native release eval --engine rust" \
  "$GEN" eval "$TMP_DIR/mod.gc" --engine rust
expect_release_rejected "native release run --engine rust" \
  "$GEN" run "$TMP_DIR/prog.gc" --engine rust --caps "$TMP_DIR/caps.toml" --log "$TMP_DIR/native.rust.gclog"
expect_release_rejected "native release policy list --coreform-frontend rust" \
  "$GEN" --coreform-frontend rust policy list --policies "$TMP_DIR/policies.toml"
expect_bootstrap_mode_parse_rejected "native release fmt --selfhost-bootstrap embedded" \
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
expect_bootstrap_mode_parse_rejected "wasi release fmt --selfhost-bootstrap embedded" \
  "$GWASI" fmt "$TMP_DIR/mod.gc" --selfhost-bootstrap embedded

echo "selfhost-release-profile-guard: ok"
