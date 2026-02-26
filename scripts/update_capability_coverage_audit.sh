#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

python3 scripts/generate_capability_coverage_audit.py \
  --root "$ROOT_DIR" \
  --host-index docs/spec/HOST_ABI_INDEX_v0.1.json \
  --prelude-index docs/spec/PRELUDE_CAPABILITY_INDEX_v0.1.json \
  --status docs/spec/CAPABILITY_COVERAGE_STATUS_v0.1.json \
  --upgrade-plan upgrade_plan.md \
  --out-json docs/spec/CAPABILITY_COVERAGE_AUDIT_v0.1.json \
  --out-md docs/spec/CAPABILITY_COVERAGE_AUDIT_v0.1.md

echo "update-capability-coverage-audit: wrote docs/spec/CAPABILITY_COVERAGE_AUDIT_v0.1.{json,md}"
