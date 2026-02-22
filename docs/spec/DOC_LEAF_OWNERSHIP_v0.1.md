# Documentation Leaf Ownership v0.1

Last updated: 2026-02-22

Purpose: define ownership and canonical source links for retained top-level
leaf docs (non-deprecated).

| Leaf doc | Canonical owner | Canonical source(s) |
|---|---|---|
| `docs/AGENT_ONBOARDING_v0.1.md` | Release Ops + Agent Tooling maintainers | `docs/spec/AGENT_AUTHORING_BUNDLE_v0.1.md`, `docs/spec/DOC_TOPOLOGY_v0.1.md` |
| `docs/PAPER_v0.2.md` | Language Architecture maintainers | `docs/spec/AGENT_AUTHORING_BUNDLE_v0.1.md`, `docs/spec/HOST_RUNTIME_BUNDLE_v0.1.md` |
| `docs/TECH_HANDOFF.md` | Runtime + CLI maintainers | `docs/spec/CLI_TOOLING_BUNDLE_v0.1.md`, `docs/spec/HOST_RUNTIME_BUNDLE_v0.1.md` |
| `docs/GETTING_STARTED.md` | Release Ops maintainers | `docs/spec/AGENT_AUTHORING_BUNDLE_v0.1.md`, `docs/spec/CLI_TOOLING_BUNDLE_v0.1.md` |
| `docs/FOUNDATION_STDLIB_v0.2.md` | Prelude + Stdlib maintainers | `docs/spec/AGENT_AUTHORING_BUNDLE_v0.1.md`, `docs/spec/TESTING_BUNDLE_v0.1.md` |
| `docs/write_genesisCode_skill.md` | Agent Authoring maintainers | `docs/spec/WRITE_GENESISCODE_SKILL_v0.1.md`, `docs/spec/WRITE_GENESISCODE_SKILL_PACK_v0.1.md` |

Rules:
- Every active top-level leaf doc in `docs/DEPRECATION_MAP_v0.1.md` must appear
  exactly once in this table.
- Canonical source entries must be existing bundle/spec docs (or explicitly
  approved canonical roots).
- Ownership must be explicit and non-empty to avoid ambiguous retrieval paths.
