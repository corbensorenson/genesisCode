# Documentation Deprecation Map v0.1

Last updated: 2026-02-21

Purpose: explicitly mark superseded/overlapping docs and point to canonical replacements.

## Deprecated Top-Level Docs

| Deprecated doc | Replacement (canonical) | Status |
|---|---|---|
| `docs/CLI_SPEC_GENESISPKG_GENESISGRAPH_v0.1.md` | `docs/spec/CLI_TOOLING_BUNDLE_v0.1.md`, `docs/spec/GCPM_BUNDLE_v0.1.md`, `docs/spec/CLI.md` | Deprecated |
| `docs/GENESISGRAPH_GENESISPKG_v0.2.md` | `docs/spec/GCPM_BUNDLE_v0.1.md`, `docs/spec/REGISTRY_POLICY.md`, `docs/spec/PATCH_SCHEMA.md` | Deprecated |
| `docs/LOCK_GENERATOR_RULESET_v0.1.md` | `docs/spec/GCPM_WORKSPACE_v0.1.md`, `docs/spec/GCPM_CLI_CONTRACT_v0.1.md` | Deprecated |
| `docs/REGISTRY_PROTOCOL_MINIMAL_v0.1.md` | `docs/spec/REGISTRY_POLICY.md`, `docs/spec/TRANSPARENCY_LOG.md` | Deprecated |
| `docs/REACHABILITY_RULES_v0.1.md` | `docs/spec/REGISTRY_POLICY.md`, `docs/spec/PATCH_SCHEMA.md` | Deprecated |
| `docs/GARBAGE_COLLECTION_RULES_v0.1.md` | `docs/spec/CLI.md` (`gc/*` commands), `docs/spec/GCPM_BUNDLE_v0.1.md` | Deprecated |

## Active Top-Level References (Not Deprecated)

- `docs/PAPER_v0.2.md` (architecture thesis)
- `docs/TECH_HANDOFF.md` (implementation handoff context)
- `docs/GETTING_STARTED.md` (local setup quickstart)
- `docs/FOUNDATION_STDLIB_v0.2.md` (language-level stdlib contract)
- `docs/STACKS_v0.2.md` (design layering reference)
- `docs/STYLE_GUIDE_v0.2.md` (authoring style reference)
- `docs/POLICY_DEFAULTS_v0.1.md` (policy defaults reference)

When an active top-level reference is replaced by a bundle/spec equivalent, this file must be updated in the same change.
