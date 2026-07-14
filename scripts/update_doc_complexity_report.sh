#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

exec bash scripts/render_doc_complexity_report.sh ".genesis/perf/doc_complexity_report.json"
