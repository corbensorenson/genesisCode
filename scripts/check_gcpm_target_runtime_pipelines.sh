#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

GENESIS_BIN="${GENESIS_BIN:-$ROOT_DIR/target/debug/genesis}"
if [[ ! -x "$GENESIS_BIN" ]]; then
  cargo build -p gc_cli >/dev/null
fi

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
  required=(
    "$bundle_root/build_manifest.gc"
    "$bundle_root/provenance.gc"
    "$bundle_root/package.toml"
    "$bundle_root/package_artifact.txt"
    "$bundle_root/runtime/runtime_contract.gc"
    "$bundle_root/runtime/boot.sh"
    "$bundle_root/runtime/smoke.sh"
  )
  for f in "${required[@]}"; do
    if [[ ! -f "$f" ]]; then
      echo "gcpm-target-runtime-pipelines: missing artifact for target=$target path=$f" >&2
      exit 1
    fi
  done

  if ! grep -q ':gcpm/runtime-runner-contract' "$bundle_root/runtime/runtime_contract.gc"; then
    echo "gcpm-target-runtime-pipelines: runtime contract kind missing for target=$target" >&2
    exit 1
  fi
  if ! grep -q '"runtime-runner-bundle-v1"' "$bundle_root/build_manifest.gc"; then
    echo "gcpm-target-runtime-pipelines: build manifest pipeline-kind missing for target=$target" >&2
    exit 1
  fi

  contract_out="$(bash "$bundle_root/runtime/boot.sh" --contract | tr -d '\n')"
  boot_out="$(bash "$bundle_root/runtime/boot.sh" --boot | tr -d '\n')"
  smoke_out="$(bash "$bundle_root/runtime/smoke.sh" | tr -d '\n')"
  if [[ "$contract_out" != "contract-ok:$target:$hash_a" ]]; then
    echo "gcpm-target-runtime-pipelines: contract lane mismatch for target=$target out=$contract_out" >&2
    exit 1
  fi
  if [[ "$boot_out" != "boot-ok:$target:$hash_a" ]]; then
    echo "gcpm-target-runtime-pipelines: boot lane mismatch for target=$target out=$boot_out" >&2
    exit 1
  fi
  if [[ "$smoke_out" != "smoke-ok:$target:$hash_a" ]]; then
    echo "gcpm-target-runtime-pipelines: smoke lane mismatch for target=$target out=$smoke_out" >&2
    exit 1
  fi
done

echo "gcpm-target-runtime-pipelines: ok targets=${targets[*]}"
