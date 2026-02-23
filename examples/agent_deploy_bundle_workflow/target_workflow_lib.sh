#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
EXAMPLE_DIR="$ROOT_DIR/examples/agent_deploy_bundle_workflow"
GENESIS_BIN="${GENESIS_BIN:-$ROOT_DIR/target/debug/genesis}"

ensure_genesis_bin() {
  if [[ -x "$GENESIS_BIN" ]]; then
    return 0
  fi
  cargo build -p gc_cli >/dev/null
}

prepare_selfhost_artifact() {
  local out_path="$1"
  local repo_art="$ROOT_DIR/selfhost/toolchain.gc"
  if [[ -f "$repo_art" ]]; then
    cp "$repo_art" "$out_path"
  else
    "$GENESIS_BIN" selfhost-artifact --out "$out_path" >/dev/null
  fi
}

run_target_deploy_workflow() {
  local target="$1"
  local lane_label="$2"

  ensure_genesis_bin

  local tmp_dir
  tmp_dir="$(mktemp -d)"
  trap "rm -rf '$tmp_dir'" EXIT

  local work_dir="$tmp_dir/work"
  cp -R "$EXAMPLE_DIR" "$work_dir"

  local artifact="$tmp_dir/selfhost_toolchain.gc"
  prepare_selfhost_artifact "$artifact"

  g() {
    "$GENESIS_BIN" --selfhost-only --selfhost-artifact "$artifact" "$@"
  }

  g pack --pkg "$work_dir/package.toml" >/dev/null

  local target_out="$work_dir/.genesis/build-targets"
  local hash_a
  local hash_b
  hash_a="$(g gcpm --caps "$work_dir/caps.toml" build --pkg "$work_dir/package.toml" --target "$target" --out-dir "$target_out" | tr -d '\n')"
  hash_b="$(g gcpm --caps "$work_dir/caps.toml" build --pkg "$work_dir/package.toml" --target "$target" --out-dir "$target_out" | tr -d '\n')"

  if [[ "$hash_a" != "$hash_b" ]]; then
    echo "agent-deploy-${lane_label}-workflow: reproducibility mismatch for target '$target': a=$hash_a b=$hash_b" >&2
    return 1
  fi

  local target_bundle_dir="$target_out/$target/$hash_a"
  local launch_script=""
  local package_file=""
  local signature_file=""
  case "$target" in
    android)
      launch_script="$target_bundle_dir/artifact/launch_android.sh"
      package_file="$target_bundle_dir/artifact/package.aab"
      signature_file="$target_bundle_dir/artifact/package.aab.sig"
      ;;
    ios)
      launch_script="$target_bundle_dir/artifact/launch_ios.sh"
      package_file="$target_bundle_dir/artifact/package.ipa"
      signature_file="$target_bundle_dir/artifact/package.ipa.sig"
      ;;
    edge)
      launch_script="$target_bundle_dir/artifact/launch_edge.sh"
      package_file="$target_bundle_dir/artifact/package.edge.wasm"
      signature_file="$target_bundle_dir/artifact/package.edge.wasm.sig"
      ;;
    service-runtime)
      launch_script="$target_bundle_dir/artifact/launch_service_runtime.sh"
      package_file="$target_bundle_dir/artifact/package.service-runtime.wasm"
      signature_file="$target_bundle_dir/artifact/package.service-runtime.wasm.sig"
      ;;
    *)
      echo "agent-deploy-${lane_label}-workflow: unsupported target '$target'" >&2
      return 2
      ;;
  esac
  local required_files=(
    "$target_bundle_dir/build_manifest.gc"
    "$target_bundle_dir/provenance.gc"
    "$target_bundle_dir/package.toml"
    "$target_bundle_dir/package_artifact.txt"
    "$launch_script"
    "$package_file"
    "$signature_file"
  )
  local missing=()
  local f
  for f in "${required_files[@]}"; do
    if [[ ! -f "$f" ]]; then
      missing+=("$f")
    fi
  done
  if [[ "${#missing[@]}" -gt 0 ]]; then
    echo "agent-deploy-${lane_label}-workflow: missing target artifact files for '$target'" >&2
    printf '  %s\n' "${missing[@]}" >&2
    return 1
  fi

  local manifest_count
  local provenance_count
  manifest_count="$(find "$target_out/$target" -name 'build_manifest.gc' | wc -l | tr -d ' ')"
  provenance_count="$(find "$target_out/$target" -name 'provenance.gc' | wc -l | tr -d ' ')"
  if [[ "$manifest_count" -lt 1 || "$provenance_count" -lt 1 ]]; then
    echo "agent-deploy-${lane_label}-workflow: expected manifest/provenance artifacts for target '$target', got manifests=$manifest_count provenance=$provenance_count" >&2
    return 1
  fi

  local boot_out
  local smoke_out
  boot_out="$(bash "$launch_script" --boot | tr -d '\n')"
  smoke_out="$(bash "$launch_script" --smoke | tr -d '\n')"
  if [[ ! "$boot_out" =~ ^boot-exec-ok:${target}:${hash_a}:[0-9a-f]{64}$ ]]; then
    echo "agent-deploy-${lane_label}-workflow: boot lane mismatch for '$target': $boot_out" >&2
    return 1
  fi
  if [[ ! "$smoke_out" =~ ^smoke-exec-ok:${target}:${hash_a}:[0-9a-f]{64}$ ]]; then
    echo "agent-deploy-${lane_label}-workflow: smoke lane mismatch for '$target': $smoke_out" >&2
    return 1
  fi

  local replay
  replay="$(printf '%s|%s|%s|%s|%s|%s' "$target" "$hash_a" "$manifest_count" "$provenance_count" "$boot_out" "$smoke_out")"
  echo "agent-deploy-${lane_label}-workflow: ok replay=$replay"
}
