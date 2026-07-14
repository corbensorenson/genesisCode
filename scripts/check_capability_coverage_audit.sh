#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

STATUS_FILE="docs/spec/CAPABILITY_COVERAGE_STATUS_v0.1.json"
AUDIT_JSON="docs/spec/CAPABILITY_COVERAGE_AUDIT_v0.1.json"
AUDIT_MD="docs/spec/CAPABILITY_COVERAGE_AUDIT_v0.1.md"

for f in "$STATUS_FILE" "$AUDIT_JSON" "$AUDIT_MD" "upgrade_plan.md" "docs/spec/HOST_ABI_INDEX_v0.1.json" "docs/spec/PRELUDE_CAPABILITY_INDEX_v0.1.json"; do
  if [[ ! -f "$f" ]]; then
    echo "capability-coverage-audit: missing required file: $f" >&2
    exit 1
  fi
done

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

python3 scripts/generate_capability_coverage_audit.py \
  --root "$ROOT_DIR" \
  --host-index docs/spec/HOST_ABI_INDEX_v0.1.json \
  --prelude-index docs/spec/PRELUDE_CAPABILITY_INDEX_v0.1.json \
  --status "$STATUS_FILE" \
  --upgrade-plan upgrade_plan.md \
  --out-json "$TMP_DIR/CAPABILITY_COVERAGE_AUDIT_v0.1.json" \
  --out-md "$TMP_DIR/CAPABILITY_COVERAGE_AUDIT_v0.1.md"

if ! cmp -s "$TMP_DIR/CAPABILITY_COVERAGE_AUDIT_v0.1.json" "$AUDIT_JSON"; then
  echo "capability-coverage-audit: stale JSON audit: $AUDIT_JSON" >&2
  diff -u "$AUDIT_JSON" "$TMP_DIR/CAPABILITY_COVERAGE_AUDIT_v0.1.json" || true
  echo "capability-coverage-audit: run scripts/update_capability_coverage_audit.sh" >&2
  exit 1
fi

if ! cmp -s "$TMP_DIR/CAPABILITY_COVERAGE_AUDIT_v0.1.md" "$AUDIT_MD"; then
  echo "capability-coverage-audit: stale Markdown audit: $AUDIT_MD" >&2
  diff -u "$AUDIT_MD" "$TMP_DIR/CAPABILITY_COVERAGE_AUDIT_v0.1.md" || true
  echo "capability-coverage-audit: run scripts/update_capability_coverage_audit.sh" >&2
  exit 1
fi

echo "capability-coverage-audit: ok"
