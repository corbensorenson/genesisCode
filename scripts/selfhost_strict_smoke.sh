#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"
export GENESIS_ALLOW_RUST_ENGINE=1

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

# capability command-group strict selfhost smoke (native CLI)
cat >"$TMP_DIR/caps.effects.toml" <<TOML
allow = [
  "core/store::put",
  "core/refs::get",
  "core/pkg::init",
  "core/pkg::list",
  "core/sync::pull",
  "core/vcs::log",
  "core/gc::pin",
]

[store]
dir = "$TMP_DIR/store.effects"

[refs]
path = "$TMP_DIR/refs.effects.gc"

[op."core/pkg::init"]
base_dir = "$TMP_DIR"
create_dirs = true

[op."core/pkg::list"]
base_dir = "$TMP_DIR"

[op."core/sync::pull"]
remote_allow = ["file://"]

[op."core/gc::pin"]
base_dir = "$TMP_DIR"
create_dirs = true
TOML

cat >"$TMP_DIR/value.gc" <<'GC'
{:smoke true}
GC

native --selfhost-only store --caps "$TMP_DIR/caps.effects.toml" put --input "$TMP_DIR/value.gc" >/dev/null
native --selfhost-only refs --caps "$TMP_DIR/caps.effects.toml" get refs/heads/main >/dev/null
native --selfhost-only pkg --caps "$TMP_DIR/caps.effects.toml" init --workspace strict-smoke --lock genesis.lock >/dev/null
native --selfhost-only pkg --caps "$TMP_DIR/caps.effects.toml" list --lock genesis.lock >/dev/null
native --selfhost-only policy list --policies "$TMP_DIR/policies.native.toml" >/dev/null
native --selfhost-only gc --caps "$TMP_DIR/caps.effects.toml" pin refs/heads/main --pins .genesis/pins.toml >/dev/null
SYNC_ROOT="$(printf '0%.0s' {1..64})"
if native --selfhost-only sync --caps "$TMP_DIR/caps.effects.toml" pull --remote "file://$TMP_DIR/remote-registry" --root "$SYNC_ROOT" >/dev/null 2>&1; then
  echo "native strict sync unexpectedly succeeded against missing remote registry" >&2
  exit 1
else
  sync_rc=$?
  if [[ "$sync_rc" -ne 20 ]]; then
    echo "native strict sync failed with unexpected exit code: $sync_rc" >&2
    exit 1
  fi
fi
if native --selfhost-only vcs --caps "$TMP_DIR/caps.effects.toml" log "$SYNC_ROOT" >/dev/null 2>&1; then
  echo "native strict vcs log unexpectedly succeeded for missing commit root" >&2
  exit 1
else
  vcs_log_rc=$?
  if [[ "$vcs_log_rc" -ne 20 ]]; then
    echo "native strict vcs log failed with unexpected exit code: $vcs_log_rc" >&2
    exit 1
  fi
fi

# package strict selfhost smoke (native CLI)
PKG_N="$TMP_DIR/pkg_native"
mkdir -p "$PKG_N"
cp tests/spec/pkg_basic/basic.gc "$PKG_N/basic.gc"
cp tests/spec/pkg_basic/caps.toml "$PKG_N/caps.toml"
cp tests/spec/pkg_basic/package.toml "$PKG_N/package.toml"
cp tests/spec/pkg_basic/pure.gcpatch "$PKG_N/pure.gcpatch"

N_PACK="$(native pack --pkg "$PKG_N/package.toml" | tr -d '\n')"
S_PACK="$(native --selfhost-only --selfhost-artifact "$ART" pack --pkg "$PKG_N/package.toml" | tr -d '\n')"
if [[ "$N_PACK" != "$S_PACK" ]]; then
  echo "native strict pack mismatch rust=$N_PACK strict=$S_PACK" >&2
  exit 1
fi
N_TYPECHECK="$(native typecheck --pkg "$PKG_N/package.toml" | tr -d '\n')"
S_TYPECHECK="$(native --selfhost-only --selfhost-artifact "$ART" typecheck --pkg "$PKG_N/package.toml" | tr -d '\n')"
if [[ "$N_TYPECHECK" != "$S_TYPECHECK" ]]; then
  echo "native strict typecheck mismatch native=$N_TYPECHECK strict=$S_TYPECHECK" >&2
  exit 1
fi
N_TEST="$(native test --pkg "$PKG_N/package.toml" | tr -d '\n')"
S_TEST="$(native --selfhost-only --selfhost-artifact "$ART" test --pkg "$PKG_N/package.toml" | tr -d '\n')"
if [[ "$N_TEST" != "$S_TEST" ]]; then
  echo "native strict test mismatch rust=$N_TEST strict=$S_TEST" >&2
  exit 1
fi
native --selfhost-only --selfhost-artifact "$ART" apply-patch "$PKG_N/pure.gcpatch" --pkg "$PKG_N/package.toml" >/dev/null
native --selfhost-only --selfhost-artifact "$ART" selfhost-dashboard --store "$TMP_DIR/store" --markdown "$TMP_DIR/SELFHOST_CUTOVER.md" >/dev/null
native --selfhost-only --selfhost-artifact "$ART" vcs hash --in "$TMP_DIR/mod.gc" --engine selfhost >/dev/null
N_VCS_HASH="$(native vcs hash --in "$TMP_DIR/mod.gc" --engine rust | tr -d '\n')"
S_VCS_HASH="$(native --selfhost-only --selfhost-artifact "$ART" vcs hash --in "$TMP_DIR/mod.gc" --engine selfhost | tr -d '\n')"
if [[ "$N_VCS_HASH" != "$S_VCS_HASH" ]]; then
  echo "native strict vcs hash mismatch rust=$N_VCS_HASH strict=$S_VCS_HASH" >&2
  exit 1
fi

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
wasi_native --selfhost-only store --caps "$TMP_DIR/caps.effects.toml" put --input "$TMP_DIR/value.gc" >/dev/null
wasi_native --selfhost-only refs --caps "$TMP_DIR/caps.effects.toml" get refs/heads/main >/dev/null
wasi_native --selfhost-only pkg --caps "$TMP_DIR/caps.effects.toml" init --workspace strict-smoke-wasi --lock genesis.wasi.lock >/dev/null
wasi_native --selfhost-only pkg --caps "$TMP_DIR/caps.effects.toml" list --lock genesis.wasi.lock >/dev/null
wasi_native --selfhost-only policy list --policies "$TMP_DIR/policies.wasi.toml" >/dev/null
if wasi_native --selfhost-only sync --caps "$TMP_DIR/caps.effects.toml" pull --remote "file://$TMP_DIR/remote-registry" --root "$SYNC_ROOT" >/dev/null 2>&1; then
  echo "WASI strict sync unexpectedly succeeded against missing remote registry" >&2
  exit 1
else
  w_sync_rc=$?
  if [[ "$w_sync_rc" -ne 20 ]]; then
    echo "WASI strict sync failed with unexpected exit code: $w_sync_rc" >&2
    exit 1
  fi
fi
wasi_native --selfhost-only gc --caps "$TMP_DIR/caps.effects.toml" pin refs/heads/main --pins .genesis/wasi.pins.toml >/dev/null
if wasi_native --selfhost-only vcs --caps "$TMP_DIR/caps.effects.toml" log "$SYNC_ROOT" >/dev/null 2>&1; then
  echo "WASI strict vcs log unexpectedly succeeded for missing commit root" >&2
  exit 1
else
  w_vcs_log_rc=$?
  if [[ "$w_vcs_log_rc" -ne 20 ]]; then
    echo "WASI strict vcs log failed with unexpected exit code: $w_vcs_log_rc" >&2
    exit 1
  fi
fi
W_N_VCS_HASH="$(wasi_native vcs hash --in "$TMP_DIR/mod.gc" --engine rust | tr -d '\n')"
W_S_VCS_HASH="$(wasi_native --selfhost-only --selfhost-artifact "$ART" vcs hash --in "$TMP_DIR/mod.gc" --engine selfhost | tr -d '\n')"
if [[ "$W_N_VCS_HASH" != "$W_S_VCS_HASH" ]]; then
  echo "WASI strict vcs hash mismatch rust=$W_N_VCS_HASH strict=$W_S_VCS_HASH" >&2
  exit 1
fi
if [[ "$N_VCS_HASH" != "$W_N_VCS_HASH" ]]; then
  echo "WASI rust vcs hash mismatch native=$N_VCS_HASH wasi=$W_N_VCS_HASH" >&2
  exit 1
fi

PKG_W="$TMP_DIR/pkg_wasi"
mkdir -p "$PKG_W"
cp tests/spec/pkg_basic/basic.gc "$PKG_W/basic.gc"
cp tests/spec/pkg_basic/caps.toml "$PKG_W/caps.toml"
cp tests/spec/pkg_basic/package.toml "$PKG_W/package.toml"
W_N_PACK="$(wasi_native pack --pkg "$PKG_W/package.toml" | tr -d '\n')"
W_S_PACK="$(wasi_native --selfhost-only --selfhost-artifact "$ART" pack --pkg "$PKG_W/package.toml" | tr -d '\n')"
if [[ "$W_N_PACK" != "$W_S_PACK" ]]; then
  echo "WASI strict pack mismatch rust=$W_N_PACK strict=$W_S_PACK" >&2
  exit 1
fi
if [[ "$N_PACK" != "$W_N_PACK" ]]; then
  echo "WASI rust pack mismatch native=$N_PACK wasi=$W_N_PACK" >&2
  exit 1
fi
W_N_TEST="$(wasi_native test --pkg "$PKG_W/package.toml" | tr -d '\n')"
W_S_TEST="$(wasi_native --selfhost-only --selfhost-artifact "$ART" test --pkg "$PKG_W/package.toml" | tr -d '\n')"
if [[ "$W_N_TEST" != "$W_S_TEST" ]]; then
  echo "WASI strict test mismatch rust=$W_N_TEST strict=$W_S_TEST" >&2
  exit 1
fi
if [[ "$N_TEST" != "$W_N_TEST" ]]; then
  echo "WASI rust test mismatch native=$N_TEST wasi=$W_N_TEST" >&2
  exit 1
fi

echo "selfhost-strict smoke: ok"
