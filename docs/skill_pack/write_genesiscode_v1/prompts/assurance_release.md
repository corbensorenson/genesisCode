# Prompt: Assurance Release

Use this prompt when strengthening requirements traceability, qualification, and release evidence.

## Required Inputs

- assurance pack/profile docs and policies
- active release profile gates
- latest assurance artifacts/reports

## Required Outputs

- standards profile delta (DO-178C/NASA/IEC) for the change
- updated assurance artifact schema references
- deterministic gate updates and evidence outputs

## Minimum Verification

- `bash scripts/check_assurance_profile_packs.sh`
- `bash scripts/check_assurance_standards_crosswalk.sh`
- `bash scripts/check_write_genesiscode_skill_distribution.sh`
