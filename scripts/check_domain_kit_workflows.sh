#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

MANIFEST="prelude/modules/manifest.toml"
if [[ ! -f "$MANIFEST" ]]; then
  echo "domain-kit-workflows: missing manifest: $MANIFEST" >&2
  exit 1
fi

required_modules=(
  "30_service_orchestration.gc"
  "31_data_pipeline.gc"
  "32_network_workflow.gc"
  "33_game_loop.gc"
)

for module in "${required_modules[@]}"; do
  if [[ ! -f "prelude/modules/$module" ]]; then
    echo "domain-kit-workflows: missing prelude module: prelude/modules/$module" >&2
    exit 1
  fi
  if command -v rg >/dev/null 2>&1; then
    if ! rg -q "\"$module\"" "$MANIFEST"; then
      echo "domain-kit-workflows: manifest missing module entry: $module" >&2
      exit 1
    fi
  else
    if ! grep -q "\"$module\"" "$MANIFEST"; then
      echo "domain-kit-workflows: manifest missing module entry: $module" >&2
      exit 1
    fi
  fi
done

check_ref() {
  local file="$1"
  local pattern="$2"
  if [[ ! -f "$file" ]]; then
    echo "domain-kit-workflows: missing workflow file: $file" >&2
    exit 1
  fi
  if command -v rg >/dev/null 2>&1; then
    if ! rg -q "$pattern" "$file"; then
      echo "domain-kit-workflows: expected '$pattern' in $file" >&2
      exit 1
    fi
  else
    if ! grep -q "$pattern" "$file"; then
      echo "domain-kit-workflows: expected '$pattern' in $file" >&2
      exit 1
    fi
  fi
}

check_ref "examples/agent_compute_workflow/workflow_run.gc" "core/kit/pipeline::run-spec"
check_ref "examples/agent_gpu_compute_workflow/workflow_run.gc" "core/kit/pipeline::run-spec"
check_ref "examples/agent_network_process_workflow/workflow_run.gc" "core/kit/network::run-http-process"
check_ref "examples/agent_long_running_gfx_loop_workflow/workflow_run.gc" "core/kit/game::run-fixed-loop"
check_ref "examples/agent_service_workflow/workflow.sh" "core/kit/service::status-v1"

echo "domain-kit-workflows: ok"
