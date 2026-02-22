# Write GenesisCode Skill Distribution Kit v1

Executable multi-agent distribution kit for GenesisCode authoring.

This package is the runnable companion to:

- `docs/spec/WRITE_GENESISCODE_SKILL_PACK_v0.1.md`
- `docs/spec/WRITE_GENESISCODE_SKILL_v0.1.md`

## Canonical Kit Root

- `docs/skill_pack/write_genesiscode_v1/manifest.json`

## Kit Contents

- Canonical prompts:
  - `docs/skill_pack/write_genesiscode_v1/prompts/backlog_slice.md`
  - `docs/skill_pack/write_genesiscode_v1/prompts/capability_addition.md`
  - `docs/skill_pack/write_genesiscode_v1/prompts/selfhost_cutover.md`
- Runnable domain recipes:
  - `docs/skill_pack/write_genesiscode_v1/recipes/service_workflow.md`
  - `docs/skill_pack/write_genesiscode_v1/recipes/game_loop_workflow.md`
  - `docs/skill_pack/write_genesiscode_v1/recipes/gpu_compute_workflow.md`
  - `docs/skill_pack/write_genesiscode_v1/recipes/package_workflow.md`
- Deterministic verification entrypoint:
  - `scripts/check_write_genesiscode_skill_distribution.sh`

## Runtime Verification Contract

When `GENESIS_WRITE_SKILL_DIST_VERIFY_RUNTIME=1`, the verification script must enforce:

- `genesis/write-genesiscode-skill-conformance-v0.1` report kind.
- Minimum conformance score from kit manifest (`>= 100` by default).
- Presence of executable workflow paths across:
  - service
  - game-loop/graphics
  - gpu-compute
  - package publish/sync

## Integration

This v1 kit is a primary AI entrypoint and must stay listed in:

- `docs/spec/AGENT_AUTHORING_BUNDLE_v0.1.md`
- `docs/AGENT_ONBOARDING_v0.1.md`
