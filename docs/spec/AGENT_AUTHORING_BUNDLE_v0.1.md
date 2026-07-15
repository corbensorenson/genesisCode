# Agent Authoring Bundle v0.1

Canonical entrypoint for AI agents authoring GenesisCode projects.

Use this bundle first; open split specs only when a task requires field-level detail.

## Included Specs

- `docs/spec/GC_AGENT_CORE_CARD_v0.3.md`
- `docs/spec/GC_AGENT_CORPUS_v0.1.json`
- `docs/spec/GC_AGENT_CORPUS_v0.1.schema.json`
- `docs/spec/GC_AGENT_PROFILE_v0.3.json`
- `docs/spec/GC_AGENT_TASK_CARDS_v0.3.md`
- `docs/spec/GC_AGENT_TASK_CARDS_v0.3.json`
- `docs/spec/GC_AGENT_SYMBOL_INDEX_v0.3.json`
- `docs/spec/CLI_TOOLING_BUNDLE_v0.1.md`
- `docs/spec/GCPM_BUNDLE_v0.1.md`
- `docs/spec/HOST_RUNTIME_BUNDLE_v0.1.md`
- `docs/spec/TESTING_BUNDLE_v0.1.md`
- `docs/spec/AGENT_INDEX_v0.1.md`
- `docs/spec/AGENT_CAPABILITY_GAUNTLET_v0.1.md`
- `docs/spec/WRITE_GENESISCODE_SKILL_v0.1.md`
- `docs/spec/WRITE_GENESISCODE_SKILL_PACK_v0.1.md`
- `docs/spec/WRITE_GENESISCODE_SKILL_PACK_v0.1.json`
- `docs/spec/WRITE_GENESISCODE_SKILL_DISTRIBUTION_v1.md`
- `docs/skill_pack/write_genesiscode_v1/manifest.json`
- `docs/write_genesisCode_skill.md`

## Legacy Split Docs (must stay marked)

- `docs/spec/CLI_JSON_SCHEMAS_v0.1.md`
- `docs/spec/GCPM_JSON_SCHEMAS_v0.1.md`
- `docs/spec/HOST_BRIDGE_PROTOCOL.md`
- `docs/spec/GPU_COMPUTE_RUNTIME_PROFILE_v0.1.md`

## Agent Guidance

- Treat this bundle as the normative retrieval root for common workflows.
- Load the compact core card, then negotiate `GC-AGENT-v0.3` before generating source; profile membership describes
  syntax and semantics but never grants a host capability.
- Declare `genesis/agent-intent-v0.1` and consume `agent-plan.plan.context_cards` rather
  than loading every domain document or selecting guidance from prompt text alone.
- Validate capabilities and contracts through `genesis --json agent-index`.
- Resolve failures through bounded `genesis --json agent-index --diagnostic <exact-code>` records from the closed, content-addressed diagnostic catalog; never route on message prose.
- Keep authoring guidance synchronized with
  `.agents/skills/genesiscode-authoring/SKILL.md`.
