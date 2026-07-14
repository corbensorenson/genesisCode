#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

PERSISTENT_REPORT_PATH="${GENESIS_REMOTE_REGISTRY_RUNTIME_PARITY_REPORT:-.genesis/perf/remote_registry_runtime_parity_report.json}"
exec bash scripts/render_remote_registry_runtime_parity_report.sh "$PERSISTENT_REPORT_PATH"
