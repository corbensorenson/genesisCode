#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
genesis_configure_cargo_target_dir "$ROOT_DIR" "gc-agent-core-card" root-host

python3 scripts/lib/gc_agent_profile.py --check
python3 scripts/lib/gc_agent_core_card.py --check
python3 scripts/lib/gc_agent_core_card.py --self-test
cargo test -p gc_cli --test gc_agent_profile_v03 language_card_symbols_and_examples_parse --locked
echo "gc-agent-core-card-contract: ok (profile=GC-AGENT-v0.3 check_mode=read_only)"
