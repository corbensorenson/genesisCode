#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPORT_PATH="${GENESIS_ASSURANCE_PROFILE_PACKS_REPORT:-$ROOT_DIR/.genesis/perf/assurance_profile_packs_report.json}"
HISTORY_PATH="${GENESIS_ASSURANCE_PROFILE_PACKS_HISTORY:-$ROOT_DIR/.genesis/perf/assurance_profile_packs_history.jsonl}"

exec bash "$ROOT_DIR/scripts/render_assurance_profile_packs_report.sh"   "$REPORT_PATH"   "$HISTORY_PATH"
