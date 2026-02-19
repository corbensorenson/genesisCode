#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

# Set to 0 to skip release-binary checks for faster local loops.
STRICT_RELEASE="${GENESIS_BOOTSTRAP_RETIREMENT_STRICT_RELEASE:-1}"

[[ "$STRICT_RELEASE" =~ ^[01]$ ]] || {
  echo "bootstrap-retirement-gate: GENESIS_BOOTSTRAP_RETIREMENT_STRICT_RELEASE must be 0 or 1" >&2
  exit 2
}

bash scripts/check_old_bootstrap_retirement.sh
bash scripts/check_rust_engine_compat.sh
bash scripts/selfhost_default_profile_guard.sh

if [[ "$STRICT_RELEASE" == "1" ]]; then
  bash scripts/selfhost_release_profile_guard.sh
fi

echo "bootstrap-retirement-gate: ok (strict_release=$STRICT_RELEASE)"
