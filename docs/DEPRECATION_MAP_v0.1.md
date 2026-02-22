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

## Deprecated Top-Level Docs

| Deprecated doc | Replacement (canonical) | Status |
|---|---|---|
| _none_ | _none_ | Deprecated stub surface eliminated (canonical bundle/spec docs only) |

## Stub Contract (Deprecated Docs)

There are currently no deprecated top-level redirect stubs.
If new temporary redirects are introduced, they must:

- include `Bundle Entry:` and `Legacy Split Doc: true` markers,
- include only redirect content (`Status`, `Canonical Replacements`, `Migration Guidance`),
- avoid owning normative sections that belong to bundle/spec docs.

## Active Top-Level References (Not Deprecated)

- `docs/PAPER_v0.2.md` (architecture thesis)
- `docs/TECH_HANDOFF.md` (implementation handoff context)
- `docs/GETTING_STARTED.md` (local setup quickstart)
- `docs/FOUNDATION_STDLIB_v0.2.md` (language-level stdlib contract)
- `docs/STACKS_v0.2.md` (design layering reference)
- `docs/STYLE_GUIDE_v0.2.md` (authoring style reference)
- `docs/POLICY_DEFAULTS_v0.1.md` (policy defaults reference)

When an active top-level reference is replaced by a bundle/spec equivalent, this file must be updated in the same change.
