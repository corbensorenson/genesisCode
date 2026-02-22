# Prompt: Backlog Slice

Use this prompt to complete the highest-impact unresolved `upgrade_plan.md` items.

## Required Inputs

- Current `upgrade_plan.md`
- Current `feature_matrix.md`
- Current `docs/status/REDTEAM_REPORT.md`

## Required Outputs

- explicit checklist of selected IDs
- code/doc changes with file paths
- deterministic verification commands and outcomes
- updated remaining-open item count

## Minimum Verification

- `bash scripts/check_write_genesiscode_skill_distribution.sh`
- `bash scripts/check_write_genesiscode_skill_conformance.sh`
