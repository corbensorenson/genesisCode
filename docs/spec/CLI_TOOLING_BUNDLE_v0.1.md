# CLI Tooling Bundle v0.1

Canonical bundle for CLI behavior and machine contracts.

Use this as the first retrieval target for agent workflows that need command
semantics, flags, and JSON envelope guarantees.

## Included Specs

- `docs/spec/CLI.md` (normative command behavior)
- `docs/spec/CLI_SCHEMA_v0.1.md` (schema shape + version contract)
- `docs/spec/CLI_JSON_SCHEMAS_v0.1.md` (JSON schema artifacts and stability rules)

## Agent Guidance

- Treat this bundle as the single CLI entrypoint.
- Expand into individual docs only when a workflow needs field-level detail.
