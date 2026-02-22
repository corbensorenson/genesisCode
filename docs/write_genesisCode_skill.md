# write_genesisCode_skill

Canonical authoring guidance for AI agents and contributors writing GenesisCode.

Canonical retrieval bundle:
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/AGENT_AUTHORING_BUNDLE_v0.1.md`

Primary source of truth:
- `/Users/corbensorenson/Documents/genesisCode/.agents/skills/genesiscode-authoring/SKILL.md`

Machine-consumable contract:
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/WRITE_GENESISCODE_SKILL_v0.1.json`

Versioned distribution pack:
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/WRITE_GENESISCODE_SKILL_PACK_v0.1.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/WRITE_GENESISCODE_SKILL_PACK_v0.1.json`

Use the skill file directly for operational guidance. This document exists as a stable project-facing pointer and onboarding entry.

## Scope

- GenesisCode language/module authoring
- Prelude/editor/tooling evolution in `.gc`
- Deterministic effects + replay discipline
- GenesisGraph/GenesisPkg patch-first, obligation-gated workflows
- Self-host cutover prioritization

## Adoption

- Agentic workflows should invoke the skill at task start for language/tooling changes.
- Human reviewers should evaluate submissions against the skill’s invariants and acceptance loop.
