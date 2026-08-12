#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

configure_genesis() {
  source scripts/lib/cargo_target_dir.sh
  genesis_configure_cargo_target_dir "$ROOT_DIR" generated-selfhost-publication root-host
  cargo build -p gc_cli --bin genesis --locked --offline
  GENESIS_BIN="$CARGO_TARGET_DIR/debug/genesis"
}

component="${1:-review}"
case "$component" in
  artifact)
    configure_genesis
    temporary="$(mktemp selfhost/.toolchain.gc.XXXXXX)"
    recovery_seed=""
    recovery_input=""
    trap 'rm -f "$temporary" ${recovery_seed:+"$recovery_seed"} ${recovery_input:+"$recovery_input"}' EXIT
    source scripts/lib/selfhost_artifact_cache.sh
    source_hash="$(selfhost_source_hash "$ROOT_DIR")"
    if ! selfhost_repo_artifact_matches_source_hash "$ROOT_DIR" "$source_hash"; then
      # Recovery is a stage0 bootstrap seed, not the self-hosted publication fixed point.
      recovery_seed="$(mktemp selfhost/.toolchain.recovery.gc.XXXXXX)"
      recovery_input="$recovery_seed.missing"
      [[ ! -e "$recovery_input" ]] || {
        echo "selfhost artifact recovery input unexpectedly exists: $recovery_input" >&2
        exit 1
      }
      "$GENESIS_BIN" selfhost-artifact \
        --selfhost-artifact "$recovery_input" \
        --recover-missing-artifact \
        --out "$recovery_seed" >/dev/null
      chmod 0644 "$recovery_seed"
      mv "$recovery_seed" selfhost/toolchain.gc
      recovery_seed=""
      recovery_input=""
    fi
    "$GENESIS_BIN" selfhost-artifact --out "$temporary" >/dev/null
    chmod 0644 "$temporary"
    mv "$temporary" selfhost/toolchain.gc
    trap - EXIT
    echo "selfhost/toolchain.gc"
    ;;
  dashboard)
    configure_genesis
    exec "$GENESIS_BIN" --selfhost-artifact selfhost/toolchain.gc \
      selfhost-dashboard --markdown docs/status/SELFHOST_CUTOVER.md
    ;;
  review)
    exec bash scripts/render_selfhost_toolchain_review.sh selfhost/toolchain.review.md
    ;;
  *)
    echo "usage: scripts/update_selfhost_toolchain_review.sh [artifact|dashboard|review]" >&2
    exit 2
    ;;
esac
