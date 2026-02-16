# write_genesisCode_skill

Canonical authoring guidance for AI agents and contributors writing GenesisCode.

Primary source of truth:
- `/Users/corbensorenson/Documents/genesisCode/.agents/skills/genesiscode-authoring/SKILL.md`

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
