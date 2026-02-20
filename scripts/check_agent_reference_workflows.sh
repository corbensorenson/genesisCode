#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

GENESIS_BIN="${GENESIS_BIN:-$ROOT_DIR/target/debug/genesis}"
if [[ ! -x "$GENESIS_BIN" ]]; then
  cargo build -p gc_cli >/dev/null
fi

echo "agent-reference-workflows: running compute workflow"
GENESIS_BIN="$GENESIS_BIN" bash examples/agent_compute_workflow/workflow.sh

echo "agent-reference-workflows: running gpu-compute workflow"
GENESIS_BIN="$GENESIS_BIN" bash examples/agent_gpu_compute_workflow/workflow.sh

echo "agent-reference-workflows: running interactive gfx+compute workflow"
GENESIS_BIN="$GENESIS_BIN" bash examples/agent_interactive_gfx_compute_workflow/workflow.sh

echo "agent-reference-workflows: running service workflow"
GENESIS_BIN="$GENESIS_BIN" bash examples/agent_service_workflow/workflow.sh

echo "agent-reference-workflows: running multi-package publish workflow"
GENESIS_BIN="$GENESIS_BIN" bash examples/agent_multi_package_publish_workflow/workflow.sh

echo "agent-reference-workflows: ok"
