#!/usr/bin/env bash
set -euo pipefail

genesis_resolve_agent_automation_context() {
  local profile="$1"
  local raw="${GENESIS_AGENT_AUTOMATION_CONTEXT:-auto}"
  case "$raw" in
    0|1)
      echo "$raw"
      return 0
      ;;
    auto)
      case "$profile" in
        prepush-standard|release-full|full-selfhost-cutover|agent-inner-loop)
          echo "1"
          ;;
        *)
          echo "0"
          ;;
      esac
      return 0
      ;;
    *)
      echo "agent-gpu-profile: GENESIS_AGENT_AUTOMATION_CONTEXT must be auto, 0, or 1" >&2
      return 2
      ;;
  esac
}

genesis_apply_agent_gpu_profile_contract() {
  local profile="$1"
  local automation_context="$2"
  local agent_gpu_profile="${GENESIS_AGENT_GPU_PROFILE:-}"
  local backend_default="${HEALTH_GPU_BACKEND_POLICY_DEFAULT:-${GENESIS_HEALTH_GPU_BACKEND_POLICY_DEFAULT:-}}"
  local compute_policy="${GENESIS_GPU_COMPUTE_BACKEND_POLICY:-}"

  if [[ "$automation_context" != "0" && "$automation_context" != "1" ]]; then
    echo "agent-gpu-profile: automation context must be 0 or 1" >&2
    return 2
  fi

  if [[ "$automation_context" == "0" && -z "$agent_gpu_profile" ]]; then
    echo "agent-gpu-profile: automation_context=0 profile=$profile selection=none (optional)"
    return 0
  fi

  if [[ -z "$agent_gpu_profile" ]]; then
    echo "agent-gpu-profile: automation context requires explicit GENESIS_AGENT_GPU_PROFILE (agent-gpu-strict|agent-gpu-fallback)" >&2
    return 2
  fi

  case "$agent_gpu_profile" in
    agent-gpu-strict)
      if [[ -n "$backend_default" && "$backend_default" != "require-device" ]]; then
        echo "agent-gpu-profile: downgrade rejected for strict profile (GENESIS_HEALTH_GPU_BACKEND_POLICY_DEFAULT=$backend_default)" >&2
        return 2
      fi
      if [[ -n "$compute_policy" && "$compute_policy" != "require-device" ]]; then
        echo "agent-gpu-profile: downgrade rejected for strict profile (GENESIS_GPU_COMPUTE_BACKEND_POLICY=$compute_policy)" >&2
        return 2
      fi
      export HEALTH_GPU_BACKEND_POLICY_DEFAULT="require-device"
      export GENESIS_HEALTH_GPU_BACKEND_POLICY_DEFAULT="require-device"
      export GENESIS_GPU_COMPUTE_BACKEND_POLICY="require-device"
      ;;
    agent-gpu-fallback)
      if [[ -n "$backend_default" && "$backend_default" != "allow-fallback" && "$backend_default" != "dev-allow-fallback" ]]; then
        echo "agent-gpu-profile: invalid backend default for fallback profile (GENESIS_HEALTH_GPU_BACKEND_POLICY_DEFAULT=$backend_default)" >&2
        return 2
      fi
      if [[ -n "$compute_policy" && "$compute_policy" != "dev-allow-fallback" && "$compute_policy" != "allow-fallback" ]]; then
        echo "agent-gpu-profile: invalid compute policy for fallback profile (GENESIS_GPU_COMPUTE_BACKEND_POLICY=$compute_policy)" >&2
        return 2
      fi
      export HEALTH_GPU_BACKEND_POLICY_DEFAULT="allow-fallback"
      export GENESIS_HEALTH_GPU_BACKEND_POLICY_DEFAULT="allow-fallback"
      export GENESIS_GPU_COMPUTE_BACKEND_POLICY="dev-allow-fallback"
      ;;
    *)
      echo "agent-gpu-profile: GENESIS_AGENT_GPU_PROFILE must be agent-gpu-strict or agent-gpu-fallback" >&2
      return 2
      ;;
  esac

  echo "agent-gpu-profile: profile=$profile automation_context=$automation_context selection=$agent_gpu_profile backend_default=$HEALTH_GPU_BACKEND_POLICY_DEFAULT compute_policy=$GENESIS_GPU_COMPUTE_BACKEND_POLICY"
}
