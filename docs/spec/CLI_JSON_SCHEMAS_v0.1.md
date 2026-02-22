> Bundle Entry: `docs/spec/CLI_TOOLING_BUNDLE_v0.1.md`
> Legacy Split Doc: Prefer the bundle entrypoint for agent retrieval; this file retains detailed, topic-local semantics.

# CLI JSON Schemas v0.1 (Non-GCPM)

This document freezes the `--json` schema IDs for `genesis` commands outside the `pkg/gcpm` surface.

`pkg/gcpm` schema IDs remain in `docs/spec/GCPM_JSON_SCHEMAS_v0.1.md`.

## Shared Envelope

All commands use the global envelope from `docs/spec/CLI.md`:

- top-level `ok` boolean
- top-level `kind` schema ID (table below)
- top-level `data` object for success
- top-level `error` object for failures
- `diagnostics_schema = "genesis/diagnostics-schema-v1"`
- `diagnostics` array (empty on success, non-empty on failure)

Failure envelopes always use:

- `kind = "genesis/error-v0.2"`

## Command -> Kind

### Core runtime commands

- `fmt` -> `genesis/fmt-v0.2`
- `eval` -> `genesis/eval-v0.2`
- `explain` -> `genesis/explain-v0.2`
- `run` -> `genesis/run-v0.2`
- `replay` -> `genesis/replay-v0.2`
- `test` -> `genesis/test-v0.2`
- `pack` -> `genesis/pack-v0.2`
- `cli-schema` -> `genesis/cli-schema-v0.1`
- `agent-index` -> `genesis/agent-index-v0.1`
- `warm` -> `genesis/warm-session-v0.1`

### Security / optimization / semantic tooling

- `keygen` -> `genesis/keygen-v0.2`
- `sign` -> `genesis/sign-v0.2`
- `transparency-verify` -> `genesis/transparency-verify-v0.2`
- `typecheck` -> `genesis/typecheck-v0.2`
- `optimize` -> `genesis/optimize-v0.2`
- `semantic-edit index` -> `genesis/semantic-edit-index-v0.1`
- `semantic-edit workspace-graph` -> `genesis/semantic-edit-workspace-graph-v0.1`
- `semantic-edit refactor-plan` -> `genesis/semantic-edit-refactor-plan-v0.1`
- `apply-patch` -> `genesis/apply-patch-v0.2`
- `verify` -> `genesis/verify-v0.2`

### Selfhost lifecycle

- `selfhost-artifact` -> `genesis/selfhost-artifact-v0.2`
- `selfhost-dashboard` -> `genesis/selfhost-dashboard-v0.2`

### Store / refs / commit

- `store put` -> `genesis/store-put-v0.2`
- `store get` -> `genesis/store-get-v0.2`
- `store has` -> `genesis/store-has-v0.2`
- `store verify` -> `genesis/store-verify-v0.2`
- `refs get` -> `genesis/refs-get-v0.1`
- `refs list` -> `genesis/refs-list-v0.1`
- `refs set` -> `genesis/refs-set-v0.1`
- `refs delete` -> `genesis/refs-delete-v0.1`
- `commit new` -> `genesis/commit-new-v0.1`
- `commit show` -> `genesis/commit-show-v0.1`

### Policy / sync / gc

- `policy list` -> `genesis/policy-list-v0.1`
- `policy show` -> `genesis/policy-show-v0.1`
- `policy set-default` -> `genesis/policy-set-default-v0.1`
- `sync pull` -> `genesis/sync-pull-v0.1`
- `sync push` -> `genesis/sync-push-v0.1`
- `gc plan` -> `genesis/gc-plan-v0.1`
- `gc run` -> `genesis/gc-run-v0.1`
- `gc pin` -> `genesis/gc-pin-v0.1`
- `gc unpin` -> `genesis/gc-unpin-v0.1`
- `gc purge` -> `genesis/gc-purge-v0.1`

### VCS

- `vcs hash` -> `genesis/vcs-hash-v0.2`
- `vcs diff` -> `genesis/vcs-diff-v0.1`
- `vcs apply` -> `genesis/vcs-apply-v0.1`
- `vcs log` -> `genesis/vcs-log-v0.1`
- `vcs blame` -> `genesis/vcs-blame-v0.1`
- `vcs why` -> `genesis/vcs-why-v0.1`
- `vcs merge3` -> `genesis/vcs-merge3-v0.1`
- `vcs resolve-conflict` -> `genesis/vcs-resolve-conflict-v0.1`

## Determinism / versioning

- Schema IDs are immutable contracts for agent workflows.
- Backward-incompatible output changes require a version bump in `kind`.
- Command aliases MUST preserve `kind` for equivalent behavior.

## CLI Schema Contract (`genesis/cli-schema-v0.1`)

`genesis cli-schema` provides a machine-readable command/option schema for
agent planning.

### Envelope

- `kind = "genesis/cli-schema-v0.1"`
- Standard CLI JSON envelope from `docs/spec/CLI.md`.

### `data` payload

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

### Profile-specific allowed values

- `runtime_profile = production`:
  - `engine` and `coreform-frontend` allowed values are `["selfhost"]`.
- `runtime_profile = parity-harness`:
  - `engine` and `coreform-frontend` allowed values are `["selfhost", "rust"]`.

### Determinism rules

- Option and subcommand entries are emitted in deterministic sorted order.
- Backward-incompatible schema changes require a `kind` version bump.
