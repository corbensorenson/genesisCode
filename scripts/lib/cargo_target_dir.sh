#!/usr/bin/env bash

# Configure a deterministic cargo target directory for script-driven build/test lanes.
# Precedence:
#  1. script-specific env override passed via $4
#  2. GENESIS_CARGO_TARGET_DIR
#  3. "$root_dir/$default_rel"
genesis_configure_cargo_target_dir() {
  local root_dir="$1"
  local context="$2"
  local default_rel="$3"
  local script_env_var="${4:-}"
  local target_dir=""

  if [[ -n "$script_env_var" ]]; then
    # shellcheck disable=SC2016
    eval 'target_dir="${'"$script_env_var"':-}"'
  fi
  if [[ -z "$target_dir" ]]; then
    target_dir="${GENESIS_CARGO_TARGET_DIR:-}"
  fi
  if [[ -z "$target_dir" ]]; then
    target_dir="$root_dir/$default_rel"
  fi

  mkdir -p "$target_dir"
  export CARGO_TARGET_DIR="$target_dir"
  echo "${context}: using CARGO_TARGET_DIR=${CARGO_TARGET_DIR}"
}
