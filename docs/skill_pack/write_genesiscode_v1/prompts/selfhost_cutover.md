# Prompt: Selfhost Cutover

Use this prompt when migrating behavior from bootstrap/runtime boundaries into `.gc`.

## Required Inputs

- strict selfhost boundary docs
- active upgrade plan IDs related to selfhost parity/cutover
- affected runtime/CLI contracts

## Required Outputs

- removed fallback/deprecated path list
- parity and replay verification evidence
- updated plan/matrix status deltas

## Minimum Verification

- `bash scripts/check_selfhost_boundary.sh --strict`
- `bash scripts/check_selfhost_toolchain_review_fresh.sh`
- `bash scripts/check_write_genesiscode_skill_distribution.sh`
