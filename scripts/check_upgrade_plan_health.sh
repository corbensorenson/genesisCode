#!/usr/bin/env bash
set -euo pipefail

export LC_ALL=C

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

PLAN_FILE="upgrade_plan.md"
if [[ ! -f "$PLAN_FILE" ]]; then
  echo "upgrade-plan-health: missing file: $PLAN_FILE"
  exit 1
fi

declared_open="$(awk -F: '/^Open checklist items:/ { gsub(/[[:space:]]/, "", $2); print $2; exit }' "$PLAN_FILE")"
if [[ -z "$declared_open" || ! "$declared_open" =~ ^[0-9]+$ ]]; then
  echo "upgrade-plan-health: could not parse integer from 'Open checklist items:' line"
  exit 1
fi

actual_open="$( (rg -n '^- \[ \]' "$PLAN_FILE" || true) | wc -l | tr -d '[:space:]' )"
if [[ "$declared_open" != "$actual_open" ]]; then
  echo "upgrade-plan-health: declared open item count does not match unchecked checklist entries"
  echo "  declared_open=$declared_open"
  echo "  actual_open=$actual_open"
  exit 1
fi

if [[ "$declared_open" -gt 0 ]]; then
  echo "upgrade-plan-health: open checklist items = $declared_open (hard-gate sweep deferred until zero)"
  echo "upgrade-plan-health: ok"
  exit 0
fi

echo "upgrade-plan-health: open checklist items = 0; enforcing hard gates"

bash scripts/check_selfhost_boundary.sh
bash scripts/check_host_abi_conformance.sh
bash scripts/check_runner_high_level_op_guard.sh
bash scripts/check_prelude_capability_coverage.sh
cargo test -p gc_cli --test cli_smoke --quiet
cargo test -p gc_cli --test cli_gcpm_selfhost_acceptance --quiet
bash scripts/check_ai_iteration_slo.sh
bash scripts/check_ai_stress_suite.sh

echo "upgrade-plan-health: ok"
