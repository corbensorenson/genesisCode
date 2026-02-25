# Prompt: Complex UI/App Stacks

Use this prompt when an agent must deliver full-stack interactive UI systems.

## Required Inputs

- target UX/runtime surfaces (browser/native/xr)
- interaction + state management requirements
- deployment topology and policy constraints

## Required Outputs

- deterministic render/input/state contracts
- integration plan for runtime and deployment lanes
- replay-safe test path across required domains

## Minimum Verification

- `bash scripts/check_agent_reference_workflows.sh`
- `bash scripts/check_agent_workflow_runtime_parity.sh`
- `bash scripts/check_write_genesiscode_skill_distribution.sh`
