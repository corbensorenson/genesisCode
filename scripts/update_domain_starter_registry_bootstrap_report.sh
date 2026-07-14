#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

PERSISTENT_REPORT_PATH="${GENESIS_DOMAIN_STARTER_REGISTRY_BOOTSTRAP_REPORT:-.genesis/perf/domain_starter_registry_bootstrap_report.json}"
exec bash scripts/render_domain_starter_registry_bootstrap_report.sh \
  "$PERSISTENT_REPORT_PATH"
