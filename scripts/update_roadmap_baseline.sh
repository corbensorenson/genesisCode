#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

CAPTURE_DATE="${GENESIS_ROADMAP_BASELINE_CAPTURE_DATE:-$(date -u +%F)}"
OUT_DIR="docs/program/evidence/roadmap-baselines"
TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/genesis-roadmap-baseline-update.XXXXXX")"
CREATED_BUNDLE=""
CREATED_PUBLIC=""
cleanup() {
  chmod 600 "$TMP_DIR/secret.key" 2>/dev/null || true
  rm -rf "$TMP_DIR"
}
rollback() {
  [[ -z "$CREATED_BUNDLE" ]] || rm -f "$CREATED_BUNDLE"
  [[ -z "$CREATED_PUBLIC" ]] || rm -f "$CREATED_PUBLIC"
  cleanup
}
trap rollback ERR INT TERM
trap cleanup EXIT

if find "$OUT_DIR" -maxdepth 1 -type f -name "roadmap-baseline-e0-${CAPTURE_DATE}-*.json" -print -quit | grep -q .; then
  echo "update-roadmap-baseline: capture date already exists and history is append-only: $CAPTURE_DATE" >&2
  exit 1
fi

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
genesis_configure_cargo_target_dir "$ROOT_DIR" "roadmap-baseline-capture" root-host
cargo build --profile selfhost-strict -p gc_runtime_bench --locked --offline
python3 scripts/lib/roadmap_baseline.py capture \
  --binary "$CARGO_TARGET_DIR/selfhost-strict/gc_runtime_bench" \
  --capture-date "$CAPTURE_DATE" \
  --output "$TMP_DIR/statement.json"

genesis_clear_resolved_cargo_target_dir "roadmap-baseline-crypto-transition"
genesis_configure_cargo_target_dir "$ROOT_DIR" "roadmap-baseline-crypto" evidence-verifier-host
cargo build --manifest-path tools/genesis-evidence-producer/Cargo.toml --locked --offline
cargo build --manifest-path tools/genesis-evidence-verifier/Cargo.toml --locked --offline --bin genesis-roadmap-baseline-verifier

python3 - "$TMP_DIR/secret.key" <<'PY'
import os
import secrets
import sys
fd = os.open(sys.argv[1], os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
with os.fdopen(fd, "wb") as handle:
    handle.write(secrets.token_bytes(32))
PY
"$CARGO_TARGET_DIR/debug/genesis-evidence-producer" \
  --statement "$TMP_DIR/statement.json" \
  --secret-key "$TMP_DIR/secret.key" >"$TMP_DIR/signature.json"

read -r baseline_id public_sha keyid <<EOF
$(python3 - "$TMP_DIR/statement.json" "$TMP_DIR/signature.json" <<'PY'
import json
import sys
statement = json.load(open(sys.argv[1], encoding="utf-8"))
signature = json.load(open(sys.argv[2], encoding="utf-8"))
print(statement["baselineIdentitySha256"], signature["publicKeySha256"], signature["keyid"])
PY
)
EOF
CREATED_BUNDLE="$OUT_DIR/roadmap-baseline-e0-${CAPTURE_DATE}-sha256-${baseline_id}.json"
CREATED_PUBLIC="$OUT_DIR/roadmap-baseline-fixture-key-sha256-${public_sha}.pub"
python3 scripts/lib/roadmap_baseline.py assemble \
  --statement "$TMP_DIR/statement.json" \
  --signature "$TMP_DIR/signature.json" \
  --output "$CREATED_BUNDLE" \
  --public-key-output "$CREATED_PUBLIC"
"$CARGO_TARGET_DIR/debug/genesis-roadmap-baseline-verifier" \
  --bundle "$CREATED_BUNDLE" \
  --public-key "$CREATED_PUBLIC" \
  --expected-keyid "$keyid" >"$TMP_DIR/verification.json"
bash scripts/update_evidence_fixture_classification.sh

trap - ERR INT TERM
echo "update-roadmap-baseline: created $CREATED_BUNDLE"
echo "update-roadmap-baseline: created $CREATED_PUBLIC"
