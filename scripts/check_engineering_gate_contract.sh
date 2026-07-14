#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

PYTHONDONTWRITEBYTECODE=1 python3 scripts/lib/engineering_gate_budgets.py check
PYTHONDONTWRITEBYTECODE=1 python3 scripts/lib/engineering_gate_budgets.py self-test
