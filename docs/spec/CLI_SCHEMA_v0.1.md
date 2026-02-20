# CLI Schema v0.1

`genesis cli-schema` provides a machine-readable command/option schema for agent planning.

## Envelope

- `kind = "genesis/cli-schema-v0.1"`
- Standard CLI JSON envelope from `docs/spec/CLI.md`.

## `data` Payload

```json
{
  "schema": "genesis/cli-schema-v0.1",
  "runtime_profile": "production|parity-harness",
  "command": {
    "name": "genesis",
    "path": ["genesis"],
    "about": "optional string",
    "options": [
      {
        "name": "coreform_frontend",
        "long": "coreform-frontend",
        "short": null,
        "help": "optional string",
        "required": false,
        "global": true,
        "positional": false,
        "value_names": ["COREFORM_FRONTEND"],
        "default_values": [],
        "allowed_values": ["selfhost"]
      }
    ],
    "subcommands": [
      {
        "name": "fmt",
        "path": ["genesis", "fmt"],
        "about": "optional string",
        "options": [],
        "subcommands": []
      }
    ]
  }
}
```

## Profile-Specific Allowed Values

- `runtime_profile = production`:
  - `engine` and `coreform-frontend` allowed values are `["selfhost"]`.
- `runtime_profile = parity-harness`:
  - `engine` and `coreform-frontend` allowed values are `["selfhost", "rust"]`.

## Determinism Rules

- Option and subcommand entries are emitted in deterministic sorted order.
- This schema is an API contract for AI workflows. Backward-incompatible changes require a version bump.
