# Plugin + FFI ABI Schemas v0.1

Schema-id contracts for typed bridge-backed host capability calls.

## Purpose

The following capability families support typed request/response schema IDs:

- `host/plugin::command`
- `editor/plugin::command`
- `host/ffi::call`
- `host/ffi::buffer-pin`
- `host/ffi::buffer-unpin`

When schema IDs are present:

- runtime performs deterministic preflight schema validation for request and response terms,
- per-op policy must define `allow_schema_ids` and include every used schema ID.

Calls without schema IDs remain backward-compatible.

## Plugin Request Schemas

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

## Plugin Response Schemas

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

## FFI Request Schemas

### `genesis/ffi.request.call.v1`

Payload must be a map:

- required:
  - `:abi-id` non-empty string|symbol
  - `:library` non-empty string|symbol
  - `:symbol` non-empty string|symbol
- optional:
  - `:payload` term
  - `:mode` string|symbol

### `genesis/ffi.request.buffer-pin.v1`

Payload must be a map:

- required:
  - `:abi-id` non-empty string|symbol
  - `:bytes` bytes|string
- optional:
  - `:read-only` bool
  - `:lifetime` string|symbol
  - `:owner` string

### `genesis/ffi.request.buffer-unpin.v1`

Payload must be a map:

- required:
  - `:abi-id` non-empty string|symbol
  - `:handle` non-empty string|symbol
- optional:
  - `:reason` string|symbol

## FFI Response Schemas

### `genesis/ffi.response.call.v1`

Response must be a map:

- required:
  - `:ok` bool
- conditional:
  - when `:ok` is true: `:result` is required
  - when `:ok` is false: `:error` map is required
    - `:error/:message` non-empty string
    - `:error/:code` string|symbol (optional)

### `genesis/ffi.response.buffer-handle.v1`

Response must be a map:

- required:
  - `:ok` bool
- conditional:
  - when `:ok` is true: `:handle` non-empty string|symbol is required
  - when `:ok` is false: `:error` map is required
    - `:error/:message` non-empty string
    - `:error/:code` string|symbol (optional)

### `genesis/ffi.response.status.v1`

Response must be a map:

- required:
  - `:ok` bool
- conditional:
  - when `:ok` is true: `:status` string|symbol is optional
  - when `:ok` is false: `:error` map is required
    - `:error/:message` non-empty string
    - `:error/:code` string|symbol (optional)

## FFI Safety Model

- Ownership is explicit and handle-based:
  - pinned memory is represented as opaque handles returned by host bridge responses.
  - raw pointers are never embedded in kernel-visible values.
- Lifetime is explicit:
  - `host/ffi::buffer-pin` admits `:lifetime` and `:owner` metadata.
  - `host/ffi::buffer-unpin` closes the handle lifecycle.
- Deterministic mode limits:
  - replay does not re-execute host native code,
  - the capability runner emits boundary envelopes with `:request-h` and `:result-h`,
  - policy must bound pin payload size with `max_buffer_bytes`.

## Prelude Wrappers

- `core/plugin::typed-command`
- `core/plugin::typed-editor-command`
- `core/editor/plugin::typed-host-command`

Legacy wrappers (`core/plugin::command`, `core/plugin::editor-command`, `core/editor/plugin::host-command`) remain valid and do not set schema IDs.
