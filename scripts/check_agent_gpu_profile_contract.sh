#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

source "$ROOT_DIR/scripts/lib/agent_gpu_profile_contract.sh"

PROFILE="${GENESIS_HEALTH_PROFILE:-dev-fast}"
AUTOMATION_CONTEXT="$(genesis_resolve_agent_automation_context "$PROFILE")"
export GENESIS_AGENT_AUTOMATION_CONTEXT="$AUTOMATION_CONTEXT"
genesis_apply_agent_gpu_profile_contract "$PROFILE" "$AUTOMATION_CONTEXT"

echo "agent-gpu-profile-contract: ok (profile=$PROFILE automation_context=$AUTOMATION_CONTEXT)"
