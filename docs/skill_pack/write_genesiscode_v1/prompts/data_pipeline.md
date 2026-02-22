# Prompt: Data Pipeline

Use this prompt when implementing durable data and IO-heavy workflows.

## Required Inputs

- `docs/spec/DOMAIN_KITS_v0.1.md`
- durable data capability docs
- relevant workflow examples and policy files

## Required Outputs

- deterministic data contract/shape updates
- policy and bounds changes for data operations
- replay-consistent workflow verification evidence

## Minimum Verification

- `bash scripts/check_agent_reference_workflows.sh`
- `bash scripts/check_feature_matrix_evidence.sh`
- `bash scripts/check_write_genesiscode_skill_distribution.sh`
