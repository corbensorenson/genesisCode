#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
genesis_configure_cargo_target_dir "$ROOT_DIR" "gc-agent-task-cards" root-host

python3 scripts/lib/gc_agent_profile.py --check
python3 scripts/lib/gc_agent_core_card.py --check
python3 scripts/lib/gc_agent_task_cards.py --check
python3 scripts/lib/gc_agent_task_cards.py --self-test
cargo test -p gc_cli --test cli_agent_plan task_cards --locked
cargo test -p gc_cli --test cli_agent_plan \
  task_cards_python_and_planner_selection_remain_stable_under_parallel_load \
  --locked -- --ignored --exact
echo "gc-agent-task-cards-contract: ok (cards=7 intent=genesis/agent-intent-v0.1 check_mode=read_only)"
