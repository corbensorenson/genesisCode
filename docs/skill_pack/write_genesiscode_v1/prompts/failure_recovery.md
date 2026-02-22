# Prompt: Failure Recovery

Use this prompt when adding fail-closed error handling, rollback, or recovery behavior.

## Required Inputs

- affected capability/effect contract docs
- failure injection gate outputs
- relevant policy profile docs

## Required Outputs

- explicit failure taxonomy and sealed error expectations
- deterministic recovery path (or fail-closed rejection) with no silent fallback
- regression tests for injected failure classes

## Minimum Verification

- `bash scripts/check_host_bridge_fault_injection.sh`
- `bash scripts/check_no_user_panics.sh`
- `bash scripts/check_write_genesiscode_skill_distribution.sh`
