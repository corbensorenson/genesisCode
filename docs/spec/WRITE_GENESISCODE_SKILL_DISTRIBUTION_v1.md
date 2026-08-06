# Write GenesisCode Skill Distribution Kit v1

Executable multi-agent distribution kit for GenesisCode authoring.

This package is the runnable companion to:

- `docs/spec/WRITE_GENESISCODE_SKILL_PACK_v0.1.md`
- `docs/spec/WRITE_GENESISCODE_SKILL_v0.1.md`

## Canonical Kit Root

- `docs/skill_pack/write_genesiscode_v1/manifest.json`

## Kit Contents

- Hand-owned workflow source: `policies/genesiscode_authoring_workflow_v0.1.json`
- Generated compact index: `docs/skill_pack/write_genesiscode_v1/authoring-card.md`
- Generated prompt registry: `docs/skill_pack/write_genesiscode_v1/prompt-cards.json`
- Generated executable recipe registry: `docs/skill_pack/write_genesiscode_v1/recipe-cards.json`
- Generated provenance manifest: `docs/skill_pack/write_genesiscode_v1/manifest.json`
- Deterministic renderer/checker: `scripts/lib/genesiscode_authoring_skill.py`
- Transactional update entrypoint: `bash scripts/update_agent_authoring_bundle.sh authoring-skill`

The registries replace per-card prose files. Their source digest and every canonical
input identity are bound in the manifest, and stale output fails closed.
- Deterministic verification entrypoint:
  - `scripts/check_write_genesiscode_skill_distribution.sh`

## Work Routing

The installed skill must preserve the closed `user-task`, `active-defect`,
`roadmap-task`, and `exploratory-work` route set. In particular, an empty
`upgrade_plan.md` disables the defect route; it never redirects an agent to wait,
invent defects, or treat status views as task authority. General roadmap work is
selected from the machine-generated execution slice, while exploration carries no
completion authority.

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
