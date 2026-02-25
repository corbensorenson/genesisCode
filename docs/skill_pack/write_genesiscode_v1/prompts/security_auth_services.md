# Prompt: Security/Auth Services

Use this prompt for security-critical auth/service boundary design.

## Required Inputs

- trust boundaries and identity/auth requirements
- service topology and policy constraints
- required audit/replay evidence obligations

## Required Outputs

- explicit capability boundary contracts
- deterministic service/auth workflow plan
- audit-ready verification and failure-path checks

## Minimum Verification

- `bash scripts/check_agent_reference_workflows.sh`
- `bash scripts/check_host_bridge_fault_injection.sh`
- `bash scripts/check_write_genesiscode_skill_distribution.sh`
