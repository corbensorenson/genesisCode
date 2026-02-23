# Documentation Deprecation Map v0.1

Last updated: 2026-02-22

Purpose: explicitly mark superseded/overlapping docs and point to canonical replacements.

## Merged Split Specs (Removed)

The following low-signal split docs were merged into canonical bundle-owned specs
and removed from active retrieval paths:

| Removed split doc | Canonical replacement |
|---|---|
| `docs/spec/CLI_SCHEMA_v0.1.md` | `docs/spec/CLI_JSON_SCHEMAS_v0.1.md` |
| `docs/spec/HOST_ABI_INDEX_v0.1.md` | `docs/spec/HOST_ABI.md` |
| `docs/spec/HOST_ABI_SCHEMA_INDEX_v0.1.md` | `docs/spec/HOST_ABI.md` |
| `docs/spec/GCPM_ABI_INDEX_v0.1.md` | `docs/spec/GCPM_JSON_SCHEMAS_v0.1.md` |
| `docs/spec/BUDGETS.md` | `docs/spec/TEST_EXECUTION_PROFILES_v0.1.md`, `docs/spec/CONCURRENCY_GPU_SLO_v0.1.md`, `docs/spec/SOURCE_SIZE_BUDGET_v0.1.md` |
| `docs/spec/COVERAGE.md` | `docs/spec/ASSURANCE_ARTIFACTS_v0.1.md`, `docs/spec/ASSURANCE_PROFILE_PACKS_v0.1.md` |

## Deprecated Top-Level Docs

| Deprecated doc | Replacement (canonical) | Status |
|---|---|---|
| `docs/POLICY_DEFAULTS_v0.1.md` | `docs/spec/ASSURANCE_PROFILE_PACKS_v0.1.md`, `docs/spec/ASSURANCE_ARTIFACTS_v0.1.md`, `docs/spec/REGISTRY_POLICY.md` | Redirect stub only |
| `docs/STACKS_v0.2.md` | `docs/PAPER_v0.2.md`, `docs/FOUNDATION_STDLIB_v0.2.md`, `docs/spec/AGENT_AUTHORING_BUNDLE_v0.1.md` | Redirect stub only |
| `docs/STYLE_GUIDE_v0.2.md` | `docs/spec/AI_STYLE.md`, `docs/spec/WRITE_GENESISCODE_SKILL_v0.1.md`, `docs/spec/WRITE_GENESISCODE_SKILL_PACK_v0.1.md` | Redirect stub only |

## Stub Contract (Deprecated Docs)

Deprecated top-level redirect stubs must:

- include `Bundle Entry:` and `Legacy Split Doc: true` markers,
- include only redirect content (`Status`, `Canonical Replacements`, `Migration Guidance`),
- avoid owning normative sections that belong to bundle/spec docs.

## Active Top-Level References (Not Deprecated)

- `docs/AGENT_ONBOARDING_v0.1.md` (agent retrieval entrypoint)
- `docs/PAPER_v0.2.md` (architecture thesis)
- `docs/TECH_HANDOFF.md` (implementation handoff context)
- `docs/GETTING_STARTED.md` (local setup quickstart)
- `docs/FOUNDATION_STDLIB_v0.2.md` (language-level stdlib contract)
- `docs/write_genesisCode_skill.md` (agent skill authoring handbook)

Ownership and canonical source links for this retained leaf-doc set are tracked in:
`docs/spec/DOC_LEAF_OWNERSHIP_v0.1.md`.

When an active top-level reference is replaced by a bundle/spec equivalent, this
file must be updated in the same change.
