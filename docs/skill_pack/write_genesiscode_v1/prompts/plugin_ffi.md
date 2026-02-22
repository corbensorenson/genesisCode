# Prompt: Plugin/FFI

Use this prompt when extending host plugin/FFI contracts or bridge semantics.

## Required Inputs

- `docs/spec/HOST_ABI.md`
- `docs/spec/PLUGIN_ABI_SCHEMAS_v0.1.md`
- affected capability wrappers and policy docs

## Required Outputs

- typed request/response schema deltas
- command allowlist + bridge digest pinning changes
- deterministic host-bridge replay expectations

## Minimum Verification

- `bash scripts/check_host_abi_conformance.sh`
- `bash scripts/check_capability_indices.sh`
- `bash scripts/check_write_genesiscode_skill_distribution.sh`
