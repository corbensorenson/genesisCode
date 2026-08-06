# Write GenesisCode Skill Contract v0.1

Machine-consumable contract for validating that the canonical GenesisCode authoring
skill remains aligned with current CLI/ABI/spec surfaces.

## Canonical Artifact

- `docs/spec/WRITE_GENESISCODE_SKILL_v0.1.json`
- Distribution pack: `docs/spec/WRITE_GENESISCODE_SKILL_PACK_v0.1.md`

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
- `required_work_routes`

## Work Selection Contract

The generated skill exposes exactly four non-interchangeable routes:

- `user-task` uses the current concrete user request as intent, subject to normative constraints.
- `active-defect` uses only open P0/P1 IDs from `upgrade_plan.md`; the route is unavailable when none exist.
- `roadmap-task` uses the bounded output of `python3 scripts/lib/roadmap_execution_manifest.py --slice`, not unchecked boxes or an empty defect ledger.
- `exploratory-work` produces quarantined observations or proposals and has no closure authority.

Every route declares its selection source, empty-source behavior, closure authority, and stop conditions. A status document may constrain or evidence work without becoming task authority.

## Drift Gate

- Gate script: `scripts/check_genesiscode_authoring_skill.sh`
- Pack conformance gate: `scripts/check_write_genesiscode_skill_pack.sh`
- Health integration: `scripts/check_upgrade_plan_health.sh` common gates

The gate fails closed when the pointer doc, skill file, required references, or
schema/index contract IDs drift out of sync.
