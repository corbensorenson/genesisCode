#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "$ROOT_DIR/examples/agent_deploy_bundle_workflow/target_workflow_lib.sh"

run_target_deploy_workflow "ios" "ios"
