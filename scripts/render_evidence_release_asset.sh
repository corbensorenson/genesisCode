#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if [[ "$#" -lt 3 || "$#" -gt 8 ]]; then
  echo "usage: scripts/render_evidence_release_asset.sh <output-dir> <E3|E4> <release-id> [bundle] [tree] [artifact-root] [trust-policy] [trust-policy-sha256]" >&2
  exit 64
fi

OUTPUT_DIR="$1"
STORAGE_CLASS="$2"
RELEASE_ID="$3"
BUNDLE="${4:-docs/program/evidence/GENESIS_EVIDENCE_BUNDLE_v0.1.json}"
TREE="${5:-docs/program/evidence/GENESIS_EVIDENCE_ARTIFACT_TREE_v0.1.json}"
ARTIFACT_ROOT="${6:-docs/program/evidence}"
TRUST_POLICY="${7:-policies/evidence_verifier_trust_v0.1.json}"
TRUST_POLICY_SHA256="${8:-6c11d747540c71887a23074f7d30b1f8eecd79b695eae9af79553d11a8011220}"
VERIFIER_MANIFEST="tools/genesis-evidence-verifier/Cargo.toml"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "evidence-release-asset" \
  evidence-verifier-host

TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/genesis-evidence-release.XXXXXX")"
trap 'rm -rf "$TMP_DIR"' EXIT

cargo build --manifest-path "$VERIFIER_MANIFEST" --locked --offline --quiet
"$CARGO_TARGET_DIR/debug/genesis-evidence-verifier" \
  --bundle "$BUNDLE" \
  --policy "$TRUST_POLICY" \
  --policy-sha256 "$TRUST_POLICY_SHA256" \
  --artifact-tree "$TREE" \
  --artifact-root "$ARTIFACT_ROOT" \
  >"$TMP_DIR/verification-result.json"

python3 scripts/lib/evidence_storage.py \
  render-release \
  --storage-class "$STORAGE_CLASS" \
  --release-id "$RELEASE_ID" \
  --bundle "$BUNDLE" \
  --artifact-tree "$TREE" \
  --artifact-root "$ARTIFACT_ROOT" \
  --verification-result "$TMP_DIR/verification-result.json" \
  --output-dir "$OUTPUT_DIR"
