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
  - `docs/skill_pack/write_genesiscode_v1/prompts/deployment_targets.md`
  - `docs/skill_pack/write_genesiscode_v1/prompts/failure_recovery.md`
  - `docs/skill_pack/write_genesiscode_v1/prompts/performance_triage.md`
  - `docs/skill_pack/write_genesiscode_v1/prompts/assurance_release.md`
  - `docs/skill_pack/write_genesiscode_v1/prompts/plugin_ffi.md`
  - `docs/skill_pack/write_genesiscode_v1/prompts/xr_experience.md`
  - `docs/skill_pack/write_genesiscode_v1/prompts/data_pipeline.md`
- Runnable domain recipes:
  - `docs/skill_pack/write_genesiscode_v1/recipes/service_workflow.md`
  - `docs/skill_pack/write_genesiscode_v1/recipes/game_loop_workflow.md`
  - `docs/skill_pack/write_genesiscode_v1/recipes/gpu_compute_workflow.md`
  - `docs/skill_pack/write_genesiscode_v1/recipes/package_workflow.md`
  - `docs/skill_pack/write_genesiscode_v1/recipes/deployment_targets_workflow.md`
  - `docs/skill_pack/write_genesiscode_v1/recipes/failure_recovery_workflow.md`
  - `docs/skill_pack/write_genesiscode_v1/recipes/performance_triage_workflow.md`
  - `docs/skill_pack/write_genesiscode_v1/recipes/assurance_workflow.md`
  - `docs/skill_pack/write_genesiscode_v1/recipes/plugin_ffi_workflow.md`
  - `docs/skill_pack/write_genesiscode_v1/recipes/xr_workflow.md`
  - `docs/skill_pack/write_genesiscode_v1/recipes/data_workflow.md`
  - `docs/skill_pack/write_genesiscode_v1/recipes/gpu_compute_workflow.md` (also used for non-graphics `gpu_data_simulation_workflow`)
  - `docs/skill_pack/write_genesiscode_v1/recipes/xr_workflow.md` (also used for `xr_deploy_test_workflow`)
- Deterministic verification entrypoint:
  - `scripts/check_write_genesiscode_skill_distribution.sh`

## Runtime Verification Contract

When `GENESIS_WRITE_SKILL_DIST_VERIFY_RUNTIME=1`, the verification script must enforce:

- `genesis/write-genesiscode-skill-conformance-v0.1` report kind.
- Minimum conformance score from kit manifest distribution requirements (`>= 100` by default).
- Minimum corpus breadth thresholds from manifest (`min_prompts`, `min_recipes`).
- Required domain coverage from manifest (`required_recipe_domains`) including:
  - service
  - game-loop/graphics
  - gpu-compute
  - gpu non-graphics compute
  - package publish/sync
  - deployment targets
  - failure recovery
  - performance triage
  - assurance
  - plugin/ffi
  - xr runtime
  - xr productization/deploy-test
  - durable data
- At least one fault-injection recipe (`mode = "fault-injection"`).

## Integration

This v1 kit is a primary AI entrypoint and must stay listed in:

- `docs/spec/AGENT_AUTHORING_BUNDLE_v0.1.md`
- `docs/AGENT_ONBOARDING_v0.1.md`
