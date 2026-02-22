> Bundle Entry: `docs/spec/HOST_RUNTIME_BUNDLE_v0.1.md`
> Legacy Split Doc: Prefer the bundle entrypoint for agent retrieval; this file retains detailed, topic-local semantics.

# HOST ABI Index v0.1

Machine-readable host ABI index for agent planning lives at:

- `/Users/corbensorenson/Documents/genesisCode/docs/spec/HOST_ABI_INDEX_v0.1.json`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/HOST_ABI_SCHEMA_INDEX_v0.1.json`

Generation + drift checks:

- regenerate: `bash scripts/update_capability_indices.sh`
- verify: `bash scripts/check_capability_indices.sh`

The JSON file is derived directly from host dispatch sources in `gc_effects` and is CI-gated.

## Schema

Top-level keys:

- `kind = "genesis/host-abi-index-v0.1"`
- `generated_from` (`string[]` of Rust source paths)
- `operations` (`string[]`, sorted unique)
- `families` (`map<string, string[]>`, sorted keys and values)

Schema index (`HOST_ABI_SCHEMA_INDEX_v0.1.json`) top-level keys:

- `kind = "genesis/host-abi-schema-index-v0.1"`
- `generated_from` (`string[]` of Rust/doc source paths)
- `operations` (`map<string, schema-entry>`)
  - `payload`:
    - `type` (usually `"map"`)
    - `required_fields` / `optional_fields` (`[{name,type,constraints?}]`)
    - `constraints` (`string[]`)
  - `response_envelope`:
    - `success` (`value_kind`, `shape`)
    - `error` (`sealed`, `code_field`, `code_prefix`)

## Canonical Family Examples

These examples define stable payload/response map shapes used by agent workflows.

- `core/store::*`
  - `core/store::put`
  - payload: `{:artifact <datum>}`
  - response: `{:hash "<64-hex>"}` or sealed `core/caps/*` error

- `core/refs::*`
  - `core/refs::get`
  - payload: `{:name "refs/heads/main"}`
  - response: `{:hash "<64-hex>"|nil}`

- `core/sync::*`
  - `core/sync::push`
  - payload: `{:remote "<remote-spec>" :refs [...] :roots [...] :depth <int>}`
  - response: `{:pushed [...]}`

- `io/fs::*`
  - `io/fs::read`
  - payload: `{:path "relative/or/absolute"}`
  - response: `<bytes>` (bounded by per-op `max_bytes` policy when configured)
  - `io/fs::stat`
  - response: `{:path <string> :exists <bool> :kind <symbol> :len-bytes <int> :readonly <bool>}`

- `sys/time::now`
  - payload: `{}`
  - response: `{:epoch-ms <int>}`

- `core/task::*`
  - `core/task::spawn`
  - payload: `{:scope "<scope>" :label "<label>" :payload <datum>}`
  - response: `{:task-id "<id>" :state "<queued|running|done|...>"}`

- `browser/storage::*`
  - `browser/storage::get`
  - payload: `{:key "scene"}`
  - response: `{:ok true :found true|false :value <term|nil>}`

- `browser/input::*`
  - `browser/input::poll`
  - payload: `{:window-id "<id>" :max-events <int>?}`
  - response: `{:ok true :events [..]}`

- `gfx/gpu::*`
  - `gfx/gpu::create-buffer`
  - payload: `{:desc {...}}`
  - response: `{:id "<gpu-resource-id>" :kind :buffer}`

- `gpu/compute::*`
  - `gpu/compute::submit`
  - payload: `{:graph {...}}`
  - response: `{:submission-id "<id>" :status "<queued|submitted|...>"}`

- `editor/task::*`
  - `editor/task::parse-module`
  - payload: `{:source "<coreform-src>" :path "<logical-path>"?}`
  - response: `{:task-id "<id>"}` then polled via `editor/task::poll`

- `editor/watch::*`
  - `editor/watch::subscribe`
  - payload: `{:path "<dir-or-file>" :recursive <bool>?}`
  - response: `{:watch-id "<id>"}`
