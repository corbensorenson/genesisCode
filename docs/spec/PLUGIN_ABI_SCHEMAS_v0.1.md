# Plugin ABI Schemas v0.1

Schema-id contracts for typed plugin capability calls.

## Purpose

`host/plugin::command` and `editor/plugin::command` support typed request/response schemas via:

- `:request-schema-id` (preferred, alias `:request-schema`)
- `:response-schema-id` (preferred, alias `:response-schema`)

When either schema id is present:

- runtime performs deterministic preflight schema validation for request and response terms,
- per-op policy must define `allow_schema_ids` and include all used schema ids.

This is a strict additive contract; calls without schema ids remain backward-compatible.

## Request Schemas

### `genesis/plugin.request.exec.v1`

Payload must be a map:

- required:
  - `:args` vector
- optional:
  - `:cwd` string
  - `:env` map<string,string>
  - `:stdin` nil|string|bytes

### `genesis/plugin.request.jsonrpc.v1`

Payload must be a map:

- required:
  - `:method` non-empty string
- optional:
  - `:params` term
  - `:id` nil|int|string

## Response Schemas

### `genesis/plugin.response.result.v1`

Response must be a map:

- required:
  - `:ok` bool
- conditional:
  - when `:ok` is true: `:error` must be absent
  - when `:ok` is false: `:error` map is required
    - `:error/:message` non-empty string
    - `:error/:code` string|symbol (optional)
- optional:
  - `:result` term

### `genesis/plugin.response.bytes.v1`

Response must be a map:

- required:
  - `:ok` bool
- conditional:
  - when `:ok` is true: `:data` string|bytes is required
  - when `:ok` is false: `:error` map is required
    - `:error/:message` non-empty string
    - `:error/:code` string|symbol (optional)

## Prelude Wrappers

- `core/plugin::typed-command`
- `core/plugin::typed-editor-command`
- `core/editor/plugin::typed-host-command`

Legacy wrappers (`core/plugin::command`, `core/plugin::editor-command`, `core/editor/plugin::host-command`) remain valid and do not set schema ids.
