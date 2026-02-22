#!/usr/bin/env bash
set -euo pipefail

genesis_release_bin_path() {
  local bin="$1"
  if [[ -z "${CARGO_TARGET_DIR:-}" ]]; then
    echo "release-bin: CARGO_TARGET_DIR must be set before resolving release binaries" >&2
    return 2
  fi
  echo "${CARGO_TARGET_DIR%/}/release/${bin}"
}

genesis_build_release_bin() {
  local package="$1"
  local bin="$2"
  cargo build --release -q -p "$package" --bin "$bin"
  local bin_path
  bin_path="$(genesis_release_bin_path "$bin")"
  if [[ ! -x "$bin_path" ]]; then
    echo "release-bin: expected executable missing after build: $bin_path" >&2
    return 1
  fi
  echo "$bin_path"
}

genesis_build_release_bins() {
  if [[ "$#" -eq 0 ]]; then
    return 0
  fi
  cargo build --release -q "$@"
}

genesis_assert_release_bin() {
  local bin="$1"
  local bin_path
  bin_path="$(genesis_release_bin_path "$bin")"
  if [[ ! -x "$bin_path" ]]; then
    echo "release-bin: expected executable missing after build: $bin_path" >&2
    return 1
  fi
  echo "$bin_path"
}
