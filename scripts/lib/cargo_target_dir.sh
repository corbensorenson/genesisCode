#!/usr/bin/env bash

# Configure a content-addressed Cargo target for a declared semantic scope.
genesis_clear_resolved_cargo_target_dir() {
  local context="${1:-cargo-cache-transition}"
  if [[ -n "${CARGO_TARGET_DIR:-}" && "${GENESIS_CARGO_CACHE_RESOLVED:-0}" != "1" ]]; then
    echo "${context}: refusing to clear arbitrary inherited CARGO_TARGET_DIR: $CARGO_TARGET_DIR" >&2
    return 2
  fi
  if [[ -n "${GENESIS_GENERATED_STATE_LEASE_TOKEN:-}" ]]; then
    if [[ -z "${GENESIS_GENERATED_STATE_ROOT:-}" ]]; then
      echo "${context}: generated-state lease is missing its repository root" >&2
      return 2
    fi
    if [[ -z "${GENESIS_GENERATED_STATE_LEASE_PID:-}" ]]; then
      echo "${context}: generated-state lease is missing its owner PID" >&2
      return 2
    fi
    if [[ "$GENESIS_GENERATED_STATE_LEASE_PID" == "$$" ]]; then
      python3 "$GENESIS_GENERATED_STATE_ROOT/scripts/lib/generated_state.py" \
        --root "$GENESIS_GENERATED_STATE_ROOT" release \
        --token "$GENESIS_GENERATED_STATE_LEASE_TOKEN" >/dev/null || return
    fi
  fi
  unset CARGO_TARGET_DIR
  unset GENESIS_CARGO_CACHE_RESOLVED
  unset GENESIS_CARGO_CACHE_SCOPE
  unset GENESIS_CARGO_CACHE_KEY_SHA256
  unset GENESIS_CARGO_CACHE_HIT
  unset GENESIS_CARGO_CACHE_EPHEMERAL
  unset GENESIS_CARGO_CACHE_RUSTC_IDENTITY_JSON
  unset GENESIS_GENERATED_STATE_ROOT
  unset GENESIS_GENERATED_STATE_LEASE_PID
  unset GENESIS_GENERATED_STATE_LEASE_TOKEN
}

# Configure a disposable Cargo target constrained to a caller-owned directory.
genesis_configure_ephemeral_cargo_target_dir() {
  if [[ "$#" -ne 3 ]]; then
    echo "ephemeral cargo target helper requires <context> <target> <containment-root>" >&2
    return 2
  fi
  local context="$1"
  local requested_target="$2"
  local containment_root="$3"
  local resolved_target=""

  resolved_target="$(python3 - "$context" "$requested_target" "$containment_root" <<'PY'
import pathlib
import sys

context = sys.argv[1]
requested = pathlib.Path(sys.argv[2])
root = pathlib.Path(sys.argv[3]).resolve(strict=True)
if not root.is_dir():
    raise SystemExit(f"{context}: ephemeral containment root must be a directory")
if not requested.is_absolute():
    raise SystemExit(f"{context}: ephemeral target must be an absolute path")
target = requested.resolve(strict=False)
if target.parent != root:
    raise SystemExit(
        f"{context}: ephemeral target must be a direct child of the report directory"
    )
if target.exists():
    raise SystemExit(f"{context}: ephemeral target must not already exist")
print(target)
PY
)" || return

  genesis_clear_resolved_cargo_target_dir "$context" || return
  CARGO_TARGET_DIR="$resolved_target"
  export CARGO_TARGET_DIR
  export GENESIS_CARGO_CACHE_RESOLVED=1
  export GENESIS_CARGO_CACHE_SCOPE=ephemeral
  export GENESIS_CARGO_CACHE_HIT=0
  export GENESIS_CARGO_CACHE_EPHEMERAL=1
  mkdir -p "$CARGO_TARGET_DIR" || return
  echo "${context}: cargo-cache scope=ephemeral target=${CARGO_TARGET_DIR} cleanup=caller"
}

genesis_configure_cargo_target_dir() {
  if [[ "$#" -ne 3 ]]; then
    echo "cargo cache helper requires <root> <context> <declared-scope>" >&2
    return 2
  fi
  local root_dir="$1"
  local context="$2"
  local scope="$3"
  local previous_target="${CARGO_TARGET_DIR:-}"
  local previous_resolved="${GENESIS_CARGO_CACHE_RESOLVED:-0}"
  local previous_scope="${GENESIS_CARGO_CACHE_SCOPE:-}"
  local previous_lease_token="${GENESIS_GENERATED_STATE_LEASE_TOKEN:-}"
  local previous_lease_root="${GENESIS_GENERATED_STATE_ROOT:-}"
  local canonical_root=""
  local expected_target=""
  local previous_relative=""
  local exports=""

  if [[ -n "${GENESIS_CARGO_TARGET_DIR:-}" ]]; then
    echo "${context}: GENESIS_CARGO_TARGET_DIR is retired; use GENESIS_CARGO_CACHE_ROOT" >&2
    return 2
  fi
  canonical_root="$(cd "$root_dir" && pwd -P)" || return
  if [[ -n "$previous_target" && "$previous_resolved" == "1" && \
        "$previous_scope" == "$scope" && -n "$previous_lease_token" && \
        "$previous_lease_root" == "$canonical_root" ]]; then
    expected_target="$(python3 "$root_dir/scripts/lib/cargo_cache.py" \
      --root "$root_dir" --scope "$scope" --format path --no-materialize)" || return
    if [[ "$previous_target" == "$expected_target" && \
          "$previous_target" == "$canonical_root"/* ]]; then
      previous_relative="${previous_target#"$canonical_root"/}"
      if python3 "$root_dir/scripts/lib/generated_state.py" \
        --root "$root_dir" validate-lease \
        --token "$previous_lease_token" \
        --path "$previous_relative" >/dev/null; then
        export GENESIS_CARGO_CACHE_HIT=1
        if declare -F genesis_gate_telemetry_event >/dev/null; then
          genesis_gate_telemetry_event cache-hit 1
        fi
        echo "${context}: cargo-cache scope=${scope} key=${GENESIS_CARGO_CACHE_KEY_SHA256} target=${CARGO_TARGET_DIR} lease=reused"
        return 0
      fi
    fi
  fi
  if [[ -n "${GENESIS_GENERATED_STATE_LEASE_TOKEN:-}" ]]; then
    genesis_clear_resolved_cargo_target_dir "${context}:lease-transition" || return
  fi
  if [[ -n "$previous_target" && "$previous_resolved" != "1" ]]; then
    echo "${context}: arbitrary inherited CARGO_TARGET_DIR is forbidden: $previous_target" >&2
    return 2
  fi
  exports="$(python3 "$root_dir/scripts/lib/cargo_cache.py" \
    --root "$root_dir" --scope "$scope" --format shell --lease-pid "$$")" || return
  eval "$exports"
  if [[ "${GENESIS_CARGO_CACHE_HIT:-0}" == "1" ]] && \
     declare -F genesis_gate_telemetry_event >/dev/null; then
    genesis_gate_telemetry_event cache-hit 1
  fi
  if [[ -n "$previous_target" && "$previous_scope" == "$scope" && \
        "$previous_target" != "$CARGO_TARGET_DIR" ]]; then
    echo "${context}: inherited resolver provenance does not match the current cache key" >&2
    return 2
  fi
  echo "${context}: cargo-cache scope=${scope} key=${GENESIS_CARGO_CACHE_KEY_SHA256} target=${CARGO_TARGET_DIR}"
}
