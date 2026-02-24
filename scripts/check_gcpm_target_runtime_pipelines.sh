#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "gcpm-target-runtime-pipelines" \
  ".genesis/build/cargo" \
  "GENESIS_GCPM_TARGET_RUNTIME_PIPELINES_CARGO_TARGET_DIR"

GENESIS_BIN="${GENESIS_BIN:-$ROOT_DIR/target/debug/genesis}"
cargo build -p gc_cli --bin genesis >/dev/null

tmp_dir="$(mktemp -d)"
cleanup() {
  rm -rf "$tmp_dir"
}
trap cleanup EXIT

workspace="$tmp_dir/runtime-pipeline"
mkdir -p "$workspace"
cat >"$workspace/caps.toml" <<'EOF_CAPS'
allow = []
EOF_CAPS
cat >"$workspace/lib.gc" <<'EOF_LIB'
(def mini::x 1)
mini::x
EOF_LIB
cat >"$workspace/package.toml" <<'EOF_PKG'
name = "mini"
version = "0.1.0"
obligations = []
dependencies = []

[[modules]]
path = "lib.gc"
EOF_PKG

"$GENESIS_BIN" pack --pkg "$workspace/package.toml" >/dev/null

targets=(ios android edge service-runtime)
for target in "${targets[@]}"; do
  hash_a="$("$GENESIS_BIN" gcpm --caps "$workspace/caps.toml" build --pkg "$workspace/package.toml" --target "$target" --out-dir "$workspace/.genesis/build-targets" | tr -d '\n')"
  hash_b="$("$GENESIS_BIN" gcpm --caps "$workspace/caps.toml" build --pkg "$workspace/package.toml" --target "$target" --out-dir "$workspace/.genesis/build-targets" | tr -d '\n')"
  if [[ "$hash_a" != "$hash_b" ]]; then
    echo "gcpm-target-runtime-pipelines: reproducibility mismatch for target=$target a=$hash_a b=$hash_b" >&2
    exit 1
  fi

  bundle_root="$workspace/.genesis/build-targets/$target/$hash_a"
  case "$target" in
    ios)
      package_rel="artifact/package.ipa"
      sig_rel="artifact/package.ipa.sig"
      launch_rel="artifact/launch_ios.gc"
      ;;
    android)
      package_rel="artifact/package.aab"
      sig_rel="artifact/package.aab.sig"
      launch_rel="artifact/launch_android.gc"
      ;;
    edge)
      package_rel="artifact/package.edge.wasm"
      sig_rel="artifact/package.edge.wasm.sig"
      launch_rel="artifact/launch_edge.gc"
      ;;
    service-runtime)
      package_rel="artifact/package.service-runtime.wasm"
      sig_rel="artifact/package.service-runtime.wasm.sig"
      launch_rel="artifact/launch_service_runtime.gc"
      ;;
    *)
      echo "gcpm-target-runtime-pipelines: unsupported target=$target" >&2
      exit 1
      ;;
  esac
  required=(
    "$bundle_root/build_manifest.gc"
    "$bundle_root/provenance.gc"
    "$bundle_root/package.toml"
    "$bundle_root/package_artifact.txt"
    "$bundle_root/$package_rel"
    "$bundle_root/$sig_rel"
    "$bundle_root/artifact/entrypoint.gc"
    "$bundle_root/$launch_rel"
  )
  for f in "${required[@]}"; do
    if [[ ! -f "$f" ]]; then
      echo "gcpm-target-runtime-pipelines: missing artifact for target=$target path=$f" >&2
      exit 1
    fi
  done

  if ! grep -q '"executable-target-bundle-v2"' "$bundle_root/build_manifest.gc"; then
    echo "gcpm-target-runtime-pipelines: build manifest pipeline-kind missing for target=$target" >&2
    exit 1
  fi

  sig_expected="$(tr -d '\r\n' < "$bundle_root/$sig_rel")"
  sig_actual="$(python3 - "$bundle_root/$package_rel" <<'PY'
import hashlib
import pathlib
import sys
print(hashlib.sha256(pathlib.Path(sys.argv[1]).read_bytes()).hexdigest())
PY
)"
  if [[ "$sig_actual" != "$sig_expected" ]]; then
    echo "gcpm-target-runtime-pipelines: artifact signature mismatch for target=$target expected=$sig_expected actual=$sig_actual" >&2
    exit 1
  fi

  adapter_src="$(cat "$bundle_root/$launch_rel")"
  if ! grep -q ":gcpm/target-exec-adapter" <<<"$adapter_src"; then
    echo "gcpm-target-runtime-pipelines: launch adapter contract missing for target=$target" >&2
    exit 1
  fi
  if ! grep -q "$(basename "$package_rel")" <<<"$adapter_src"; then
    echo "gcpm-target-runtime-pipelines: launch adapter package reference missing for target=$target" >&2
    exit 1
  fi
  if ! grep -q "$(basename "$sig_rel")" <<<"$adapter_src"; then
    echo "gcpm-target-runtime-pipelines: launch adapter signature reference missing for target=$target" >&2
    exit 1
  fi
  if ! grep -q "entrypoint.gc" <<<"$adapter_src"; then
    echo "gcpm-target-runtime-pipelines: launch adapter entrypoint reference missing for target=$target" >&2
    exit 1
  fi

  boot_eval="$("$GENESIS_BIN" eval "$bundle_root/artifact/entrypoint.gc" | tr -d '\n')"
  boot_h="$(python3 - "$boot_eval" <<'PY'
import hashlib
import sys
print(hashlib.sha256(sys.argv[1].encode("utf-8")).hexdigest())
PY
)"
  boot_out="boot-exec-ok:${target}:${hash_a}:${boot_h}"
  if [[ ! "$boot_out" =~ ^boot-exec-ok:${target}:${hash_a}:[0-9a-f]{64}$ ]]; then
    echo "gcpm-target-runtime-pipelines: boot lane mismatch for target=$target out=$boot_out" >&2
    exit 1
  fi

  smoke_a="$("$GENESIS_BIN" eval "$bundle_root/artifact/entrypoint.gc" | tr -d '\n')"
  smoke_b="$("$GENESIS_BIN" eval "$bundle_root/artifact/entrypoint.gc" | tr -d '\n')"
  if [[ "$smoke_a" != "$smoke_b" ]]; then
    echo "gcpm-target-runtime-pipelines: smoke nondeterministic for target=$target" >&2
    exit 1
  fi
  smoke_h="$(python3 - "$smoke_a" <<'PY'
import hashlib
import sys
print(hashlib.sha256(sys.argv[1].encode("utf-8")).hexdigest())
PY
)"
  smoke_out="smoke-exec-ok:${target}:${hash_a}:${smoke_h}"
  if [[ ! "$smoke_out" =~ ^smoke-exec-ok:${target}:${hash_a}:[0-9a-f]{64}$ ]]; then
    echo "gcpm-target-runtime-pipelines: smoke lane mismatch for target=$target out=$smoke_out" >&2
    exit 1
  fi
done

echo "gcpm-target-runtime-pipelines: ok targets=${targets[*]}"
