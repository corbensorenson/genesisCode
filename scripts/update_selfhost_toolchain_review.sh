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
    trap 'rm -f "$temporary"' EXIT
    source scripts/lib/selfhost_artifact_cache.sh
    source_hash="$(selfhost_source_hash "$ROOT_DIR")"
    artifact_args=(selfhost-artifact --out "$temporary")
    if ! selfhost_repo_artifact_matches_source_hash "$ROOT_DIR" "$source_hash"; then
      artifact_args+=(--recover-missing-artifact)
    fi
    "$GENESIS_BIN" "${artifact_args[@]}" >/dev/null
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
