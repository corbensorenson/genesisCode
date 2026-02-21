> Bundle Entry: `docs/spec/GCPM_BUNDLE_v0.1.md`
> Legacy Split Doc: Prefer the bundle entrypoint for agent retrieval; this file retains detailed, topic-local semantics.

# GCPM CLI Contract v0.1

This document freezes the first AI-agent-facing contract for GenesisCode project management.

## Naming

- `genesis pkg` remains supported as the stable compatibility surface.
- `genesis gcpm` is a first-class alias to `genesis pkg`.
- Both entrypoints MUST execute identical semantics for the same arguments.

## Scope

`gcpm/pkg` covers:
- workspace bootstrap and migration (`new`, `migrate`)
- workspace lock lifecycle (`init`, `add`, `lock`, `update`, `install`)
- lock hygiene (`remove`, `doctor`, `verify`)
- dependency inspection (`list`, `info`)
- ABI/introspection export for autonomous planning (`abi`)
- deterministic diagnostics (`doctor`)
- workspace task execution (`run`)
- package obligation execution alias (`test`)
- regulated assurance artifact emission (`trace`, `qualify`)
- deterministic profile environment realization (`env`)
- snapshot and distribution (`snapshot`, `export`, `import`, `publish`)

## JSON Stability Rules

For AI automation, JSON response envelopes are normative:

1. `--json` output MUST keep the same `kind` string regardless of using `pkg` or `gcpm` alias.
2. Existing `kind` values are versioned and treated as schema IDs.
3. Any backward-incompatible shape change requires a new `kind` version suffix.
4. Error JSON must retain machine-parseable `code` and deterministic `message` fields.

Command-to-kind schema IDs are frozen in `docs/spec/GCPM_JSON_SCHEMAS_v0.1.md`.
Workflow-level `data.report` artifacts for lock/update/publish are frozen in
`docs/spec/GCPM_WORKFLOW_REPORTS_v0.1.md`.

## Determinism Rules

- `pkg` and `gcpm` command aliases MUST produce byte-equivalent lockfile output for identical inputs.
- Effectful operations must keep deterministic `.gclog` ordering and replay behavior unchanged across aliases.
- `gcpm run` `cmd="contract"` tasks MUST be hash-pinned (`--contract-h <hex64>`) and fail closed on file hash mismatch before execution.

## Acceptance

The following must hold in CI:
- selfhost-only runs accept `gcpm` alias command paths.
- alias paths preserve existing package manager capability requirements.
- alias paths do not introduce new Rust-only fallbacks.
