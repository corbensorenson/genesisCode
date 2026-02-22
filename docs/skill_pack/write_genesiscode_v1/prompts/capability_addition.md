# Prompt: Capability Addition

Use this prompt when adding or extending host capability wrappers/contracts.

## Required Inputs

- host ABI capability op contract docs
- capability index docs/contracts
- affected prelude module(s) under `prelude/modules/`

## Required Outputs

- deterministic wrapper contract and payload schema
- fail-closed policy requirements
- replay determinism evidence path
- updated docs/spec + capability indices

## Minimum Verification

- `bash scripts/check_capability_indices.sh`
- `bash scripts/check_host_abi_conformance.sh`
- `bash scripts/check_write_genesiscode_skill_distribution.sh`
