#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "check-warm-protocol-contract" \
  root-host

python3 scripts/lib/warm_protocol_contract.py --check
python3 scripts/lib/warm_protocol_contract.py --self-test
cargo test -p gc_cli_driver warm_ --lib --quiet
cargo test -p gc_cli_driver 'mcp::' --lib --quiet
cargo test -p gc_cli --test cli_warm --quiet
cargo test -p gc_cli --test cli_mcp --quiet
cargo test -p gc_cli --test cli_agent_session --quiet

echo "warm-mcp-protocol-contract: runtime ok"
