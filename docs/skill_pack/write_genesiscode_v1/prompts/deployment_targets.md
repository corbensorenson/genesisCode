# Prompt: Deployment Targets

Use this prompt when extending `gcpm build` target coverage and deployment contracts.

## Required Inputs

- `docs/spec/CLI.md` target contract section
- `docs/spec/GCPM_JSON_SCHEMAS_v0.1.md`
- existing target tests in `crates/gc_cli/tests/cli_pkg_workspace.rs`

## Required Outputs

- target-specific artifact contract deltas
- deterministic per-target verification steps
- updated test/gate coverage proving target behavior

## Minimum Verification

- `cargo test -p gc_cli --test cli_pkg_workspace gcpm_build_supports_mobile_and_edge_target_contracts -- --exact`
- `bash scripts/check_capability_evidence_ledger.sh`
- `bash scripts/check_write_genesiscode_skill_distribution.sh`
