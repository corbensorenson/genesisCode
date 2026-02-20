# HOST ABI Index v0.1

Machine-readable host ABI index for agent planning lives at:

- `/Users/corbensorenson/Documents/genesisCode/docs/spec/HOST_ABI_INDEX_v0.1.json`

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
  - payload: `{:path "relative/or/absolute" :limit-bytes <int>?}`
  - response: `{:data <bytes> :path "<resolved>"}` (bounded by policy)

- `sys/time::now`
  - payload: `{}`
  - response: `{:epoch-ms <int>}`

- `core/task::*`
  - `core/task::spawn`
  - payload: `{:scope "<scope>" :label "<label>" :payload <datum>}`
  - response: `{:task-id "<id>" :state "<queued|running|done|...>"}`

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
