# Write GenesisCode Skill Contract v0.1

Machine-consumable contract for validating that the canonical GenesisCode authoring
skill remains aligned with current CLI/ABI/spec surfaces.

## Canonical Artifact

- `docs/spec/WRITE_GENESISCODE_SKILL_v0.1.json`

## Purpose

- Provide a stable schema/checklist payload usable by Codex, Claude Code, and other
  agent systems.
- Enforce drift checks between:
  - `docs/write_genesisCode_skill.md` pointer guidance
  - `.agents/skills/genesiscode-authoring/SKILL.md`
  - CLI schema docs and capability index specs

## JSON Contract Fields

- `kind = "genesis/write-genesiscode-skill-contract-v0.1"`
- `version`
- `bundle_entrypoint`
- `pointer_doc`
- `skill_file`
- `required_skill_sections`
- `required_spec_refs`
- `required_contract_ids`
- `required_capability_indices`
- `required_schema_docs`

## Drift Gate

- Gate script: `scripts/check_genesiscode_authoring_skill.sh`
- Health integration: `scripts/check_upgrade_plan_health.sh` common gates

The gate fails closed when the pointer doc, skill file, required references, or
schema/index contract IDs drift out of sync.
