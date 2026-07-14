#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

exec bash scripts/render_cargo_target_dir_policy_report.sh \
  ".genesis/perf/cargo_target_dir_policy_report.json" \
  ".genesis/perf/cargo_target_dir_policy_history.jsonl"
