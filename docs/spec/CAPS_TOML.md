> Bundle Entry: `docs/spec/HOST_RUNTIME_BUNDLE_v0.1.md`
> Legacy Split Doc: Prefer the bundle entrypoint for agent retrieval; this file retains detailed, topic-local semantics.

# `caps.toml` (Capability Policy) v0.2

This file defines the *deny-by-default* capability policy used by `genesis run` and by effectful obligations.

## Top-Level Keys

- `allow` (required): array of strings. Each string is a fully-qualified op symbol, e.g. `"sys/time::now"`.
- `log` (optional): table controlling effect log behavior (see below).
- `store` (optional): table controlling the artifact store used by `core/store::*` capabilities (see below).
- `refs` (optional): table controlling the local refs database used by `core/refs::*` capabilities (see below).
- `task` (optional): table controlling task scheduler defaults and limits for `core/task::*` (see below).
- `runtime` (optional): deterministic runtime budgets for effect programs (see below).

Example:
```toml
allow = ["sys/time::now", "io/fs::read"]
```

## Store Policy (`[store]`)

Supported keys:
- `dir` (string): directory used for content-addressed artifacts for `core/store::*`.
  - If omitted, defaults to `<caps.toml directory>/.genesis/store`.
- `max_run_bytes` (int, optional): cumulative byte budget for store writes during a single `genesis run`.
  - Applies to `core/store::put`, remote cache writes from `core/store::get`, sync/gpk ingest, and log artifact externalization.
  - Exceeding this budget returns sealed ERROR `core/caps/resource-limit`.
- `remote` (string, optional): remote registry base used as a read-through source for `core/store::{has,get}`.
  - If set, the runner may query/download artifacts from the remote when they are missing locally.
  - Remote normalization and allowlisting are enforced (see below).
- `remote_allow` (array of strings, optional): allowlist of normalized remote base URL prefixes permitted for `store.remote`.
  - If `store.remote` is set, `store.remote_allow` must be non-empty or the remote is denied.
- `allow_http` (bool, optional): if true, `http://` remotes are permitted (default false).
- `auth_token` (string, optional): bearer token for remote registry auth.
- `auth_token_env` (string, optional): env var name containing bearer token (mutually exclusive with `auth_token`).
- `mtls_ca_pem` (string, optional): PEM file path for additional trusted CA roots.
- `mtls_identity_pem` (string, optional): PEM file path containing client cert+key for mTLS.

Example:
```toml
[store]
dir = "./.genesis/store"
max_run_bytes = 16777216
remote = "gen://registry.example.com/registry"
remote_allow = ["https://registry.example.com/registry/v1/"]
auth_token_env = "GENESIS_REGISTRY_TOKEN"
mtls_ca_pem = "./certs/registry-ca.pem"
mtls_identity_pem = "./certs/client-identity.pem"
```

Remote normalization and matching:
- `gen://host/path` is normalized to `https://host/path`.
- Remotes are normalized to a `.../v1/` base (e.g. `https://example.com/registry/v1/`).
- `remote_allow` is matched by prefix against the normalized base.

## Refs Policy (`[refs]`)

Supported keys:
- `path` (string): local refs database file used by `core/refs::*`.
  - If omitted, defaults to `<caps.toml directory>/.genesis/refs.gc`.

Example:
```toml
[refs]
path = "./.genesis/refs.gc"
```

## Task Policy (`[task]`)

Supported keys:
- `default_workers` (int >= 1, optional): default worker budget used when `task.max_workers` is unset.
  - default when omitted: host parallelism (`available_parallelism`) with minimum `1`.
- `max_workers` (int >= 0, optional): hard worker ceiling.
- `max_tasks` (int >= 0, optional): maximum concurrently tracked tasks.
- `max_queue` (int >= 0, optional): maximum queued (not yet running) tasks.
- `max_steps_per_task` (int >= 0, optional): logical-step ceiling per task.
- `max_time_ms_per_task` (int >= 0, optional): logical elapsed-step budget per task.

Example:
```toml
[task]
default_workers = 8
max_workers = 16
max_tasks = 128
max_queue = 256
max_steps_per_task = 100000
max_time_ms_per_task = 10000
```

## Runtime Policy (`[runtime]`)

Supported keys:
- `max_effect_ops` (int >= 0, optional): maximum number of effect requests processed in a single run.
  - Exceeding the limit returns sealed ERROR `core/caps/resource-limit`.
- `max_payload_bytes_per_op` (int >= 0, optional): maximum canonical payload size (bytes) for a single effect request.
- `max_payload_bytes_per_run` (int >= 0, optional): cumulative canonical payload-byte budget for the full run.
- `max_response_bytes_per_op` (int >= 0, optional): maximum canonical response size (bytes) for a single effect response.
- `max_response_bytes_per_run` (int >= 0, optional): cumulative canonical response-byte budget for the full run.

Behavior:
- Payload/response sizes are computed from canonical CoreForm serialization, not host RSS/process memory.
- Limits are fail-closed and deterministic: the runner returns a sealed `core/caps/resource-limit` error and records a denied decision for that entry.
- Runtime limit errors include `:runtime/budget`, `:runtime/unit`, `:runtime/observed`, and `:runtime/limit` in `:error/context`.

Example:
```toml
[runtime]
max_effect_ops = 20000
max_payload_bytes_per_op = 65536
max_payload_bytes_per_run = 8388608
max_response_bytes_per_op = 1048576
max_response_bytes_per_run = 16777216
```

## Log Policy (`[log]`)

Supported keys:
- `inline_max_bytes` (int): maximum number of bytes to inline inside `.gclog` `:resp` entries.
  - If a response exceeds this limit, the runner stores the response as a content-addressed artifact and records an artifact reference in the log.
- `store_dir` (string): directory used for content-addressed artifacts referenced by logs.
  - If omitted and `inline_max_bytes` is set, `store_dir` defaults to `<caps.toml directory>/.genesis/store`.
- `max_artifact_bytes_per_run` (int, optional): cumulative byte budget for log response artifacts externalized to store in a single run.
  - Exceeding this budget returns sealed ERROR `core/caps/resource-limit`.

Example:
```toml
[log]
inline_max_bytes = 1048576
store_dir = "./.genesis/store"
max_artifact_bytes_per_run = 8388608
```

## Per-Op Configuration

Some ops may accept a per-op policy object. This is represented as a TOML table keyed by op symbol.

Supported keys:
- `base_dir` (string): base directory sandbox for `io/fs::*` ops. Paths must remain under this directory after canonicalization.
- `create_dirs` (bool): if true, `io/fs::write` and `io/fs::rename` may create parent directories.
- `timeout_ms` (int): optional runner-side timeout (milliseconds). Only supported for non-mutating ops.
- `log_inline_max_bytes` (int): optional per-op override for log inlining.
- `bridge_cmd` (string): optional host-bridge executable path under `base_dir`.
  - used by host-integrated ops such as `host/plugin::command`,
    `editor/plugin::command`,
    `gfx/window::*`, `gfx/input::*`, `gfx/audio::*`, `gfx/gpu::*`,
    `gpu/compute::*`, `io/net::*`, `io/db::*`, and `sys/process::*`.
  - first-party runtime domains (canonical `gpu/compute::*`,
    `gfx/gpu::*`, `gfx/window::*`, `gfx/input::*`, `gfx/audio::*`,
    `editor/clipboard::*`, `editor/dialog::*`, `editor/watch::*`,
    `editor/task::*`) do not require `bridge_cmd`; bridge remains an explicit override.
- `bridge_args` (array<string>): optional fixed args passed to `bridge_cmd` before the op symbol.
- `bridge_transport` (string): optional transport mode for bridge-backed ops.
  - supported values:
    - `spawn-per-op` (default): spawn a new bridge process for each op request.
    - `persistent-stdio`: keep a per-op bridge process/session alive and exchange framed request/response payloads over persistent stdio.
  - `persistent-stdio` requires the bridge executable to support repeated framed request processing in a single process lifetime.
- `first_party_profile` (string): optional profile selector for first-party host backends.
  - currently used by `gfx/window::*`, `gfx/input::*`, `gfx/audio::*`.
  - supported values:
    - `headless` (default): deterministic no-event CI/runtime profile.
    - `interactive`: host-integrated terminal adapter profile (`terminal-host`) for local window/input/audio interactivity.
    - `desktop`: non-terminal desktop adapter profile (`desktop-host`) for local window/input/audio workflows.
- `gpu_backend` (string): optional backend selector for first-party GPU runtime domains (`gpu/compute::*`, `gfx/gpu::*`).
  - supported values:
    - `first-party-runtime` (default): deterministic in-memory runtime backend.
    - `device-runtime`: in-repo device-backed backend for submit/introspection ops (`submit`, `limits`, `features`).
    - `device-runtime-full`: in-repo device-backed backend request for canonical lifecycle ops (`create*`, `write*`, `read*`, `destroy-resource`, `submit`, `limits`, `features`).
    - legacy aliases accepted and normalized:
      - `device-bridge` -> `device-runtime`
      - `device-runtime-submit` -> `device-runtime`
      - `device-runtime-lifecycle` -> `device-runtime-full`
  - applies only when no explicit bridge profile is configured for the op.
- `gpu_backend_policy` (string): optional fail behavior for `gpu_backend = "device-runtime"` or `"device-runtime-full"`.
  - supported values:
    - `allow-fallback` (default): on device backend unavailability/error, fail open to `first-party-runtime` and annotate response with fallback metadata.
    - `require-device`: fail closed with sealed error when device backend is unavailable/errors.
- `bridge_cmd_allowlist` (array<string>): optional explicit identity allowlist for bridge binaries.
  - entries may match configured `bridge_cmd`, resolved absolute path, or executable filename.
- `bridge_cmd_sha256` (string): optional executable digest pin (64 hex; optional `sha256:` prefix).
  - mismatches are denied with deterministic sealed error `<family>/bridge-identity-denied`.
- `wasi_bridge_profile` (bool): when true, enables deterministic WASI bridge response mode for this op (also always enabled on actual WASI targets).
- `wasi_bridge_response` (string): optional CoreForm term used as deterministic host response for bridge-backed ops under WASI bridge profile.
- `wasi_bridge_response_file` (string): optional path (under `base_dir`) to a CoreForm term or op->response map used under WASI bridge profile.
- `max_bytes` (int): optional per-op byte budget for payload-heavy operations.
  - `core/store::put`: maximum artifact byte size accepted for each put request.
  - `core/store::get` and `io/fs::read`: maximum bytes allowed in the fetched/read payload.
  - bridge-backed ops (`editor/*`, `gfx/*`, `gpu/compute::*`): maximum bytes for both framed
    request payload and framed response payload.
- `remote_allow` (array of strings): allowlist of remote base URL prefixes for `core/sync::*` and `core/pkg-low::publish` (see below).
- `url_allow` (array of strings): URL prefix allowlist for network target ops (`io/net::http-request`, `io/net::ws-open`, `io/net::tcp-open`, `io/net::tcp-listen`, `io/net::udp-bind`, `io/net::udp-send`, `io/net::dns-resolve`, `io/net::http-listen`).
- `allow_http` (bool): if true, `http://` URLs are permitted for `core/sync::*`, `core/pkg-low::publish`, and `io/net::http-*` (default is false).
- `wasi_network_profile` (string): optional WASI network scope (`none|local|preview2`) for remote/network ops such as `core/sync::*`, `core/pkg-low::publish`, and `io/net::*`.
- `allow_bind_hosts` (array<string>): required bind-host allowlist for inbound network listeners (`io/net::tcp-listen`, `io/net::http-listen`).
- `allow_bind_ports` (array<int>): required bind-port allowlist for inbound network listeners (`io/net::tcp-listen`, `io/net::http-listen`).
- `max_request_bytes` (int): required positive request-size bound for inbound accept/listen flows (`io/net::tcp-accept`, `io/net::http-listen`, `io/net::ws-accept`).
- `db_target_allow` (array<string>): allowlist of durable-data targets (DSN/path prefixes) for `io/db::connect` and `io/db::kv-open`.
- `allow_query_classes` (array<string>): required query/statement class allowlist for SQL-like durable-data ops (`io/db::query`, `io/db::exec`).
- `max_row_count` (int): required positive row-count bound for `io/db::query`.
- `max_result_bytes` (int): required positive result envelope byte bound for `io/db::query`, `io/db::exec`, and `io/db::kv-get`.
- `max_value_bytes` (int): required positive value-size bound for `io/db::kv-put`.
- `allow_programs` (array<string>): required allowlist for process launch ops (`sys/process::exec`, `sys/process::spawn`) program names.
- `allow_plugins` (array<string>): required allowlist for `host/plugin::command` and `editor/plugin::command` plugin identifiers.
- `allow_commands` (array<string>): optional command allowlist for `host/plugin::command` and `editor/plugin::command`.
- `allow_schema_ids` (array<string>): required when typed plugin schemas are used (`:request-schema-id` / `:response-schema-id`); every schema id must be allowlisted.
- `auth_token` (string): optional bearer token for remote auth.
- `auth_token_env` (string): optional env var name for bearer token (mutually exclusive with `auth_token`).
- `mtls_ca_pem` (string): optional PEM path for trusted CA roots.
- `mtls_identity_pem` (string): optional PEM path for client cert+key.

Note: the effect log (`.gclog`) does not record `base_dir` values.

Example:
```toml
allow = ["io/fs::read", "io/fs::write"]

[op."io/fs::read"]
base_dir = "./sandbox"
timeout_ms = 250

[op."io/fs::write"]
base_dir = "./sandbox"
create_dirs = true

[op."host/plugin::command"]
base_dir = "./workspace"
bridge_cmd = "./tools/editor_bridge.sh"
bridge_args = ["--mode", "stdio-coreform"]
allow_plugins = ["demo-plugin"]
allow_commands = ["run", "health"]
allow_schema_ids = [
  "genesis/plugin.request.exec.v1",
  "genesis/plugin.response.result.v1",
]

[op."gfx/gpu::create-buffer"]
base_dir = "./workspace"
bridge_cmd = "./tools/host_bridge.sh"

[op."gfx/window::create-surface"]
base_dir = "./workspace"
bridge_cmd = "./tools/host_bridge.sh"

[op."io/net::http-request"]
url_allow = ["https://registry.example.com/api/"]
wasi_network_profile = "preview2"
bridge_cmd = "./tools/host_bridge.sh"

[op."io/net::ws-open"]
url_allow = ["wss://realtime.example.com/ws/"]
wasi_network_profile = "preview2"
bridge_cmd = "./tools/host_bridge.sh"

[op."io/net::ws-send"]
bridge_cmd = "./tools/host_bridge.sh"

[op."io/net::ws-recv"]
bridge_cmd = "./tools/host_bridge.sh"

[op."io/net::ws-close"]
bridge_cmd = "./tools/host_bridge.sh"

[op."io/net::tcp-listen"]
url_allow = ["tcp://127.0.0.1:9000"]
allow_bind_hosts = ["127.0.0.1"]
allow_bind_ports = [9000]
wasi_network_profile = "preview2"
max_request_bytes = 4096
bridge_cmd = "./tools/host_bridge.sh"

[op."io/net::tcp-accept"]
max_request_bytes = 4096
bridge_cmd = "./tools/host_bridge.sh"

[op."io/net::http-listen"]
url_allow = ["http://127.0.0.1:8080"]
allow_http = true
allow_bind_hosts = ["127.0.0.1"]
allow_bind_ports = [8080]
wasi_network_profile = "preview2"
max_request_bytes = 8192
bridge_cmd = "./tools/host_bridge.sh"

[op."io/net::http-respond"]
bridge_cmd = "./tools/host_bridge.sh"

[op."io/net::ws-accept"]
max_request_bytes = 4096
bridge_cmd = "./tools/host_bridge.sh"

[op."io/db::connect"]
db_target_allow = ["sqlite://data/app.db"]
bridge_cmd = "./tools/host_bridge.sh"

[op."io/db::tx-begin"]
bridge_cmd = "./tools/host_bridge.sh"

[op."io/db::query"]
allow_query_classes = ["read-only", "analytics"]
max_row_count = 500
max_result_bytes = 8192
bridge_cmd = "./tools/host_bridge.sh"

[op."io/db::exec"]
allow_query_classes = ["write", "ddl"]
max_result_bytes = 4096
bridge_cmd = "./tools/host_bridge.sh"

[op."io/db::tx-commit"]
bridge_cmd = "./tools/host_bridge.sh"

[op."io/db::tx-rollback"]
bridge_cmd = "./tools/host_bridge.sh"

[op."io/db::kv-open"]
db_target_allow = ["kv://state/main"]
bridge_cmd = "./tools/host_bridge.sh"

[op."io/db::kv-get"]
max_result_bytes = 4096
bridge_cmd = "./tools/host_bridge.sh"

[op."io/db::kv-put"]
max_value_bytes = 4096
bridge_cmd = "./tools/host_bridge.sh"

[op."io/db::kv-delete"]
bridge_cmd = "./tools/host_bridge.sh"

[op."sys/process::exec"]
allow_programs = ["gcpm", "genesis-lsp"]
bridge_cmd = "./tools/host_bridge.sh"
```

## Sync/Publish Remotes (`core/sync::*`, `core/pkg-low::publish`)

`core/sync::pull`, `core/sync::push`, and `core/pkg-low::publish` are **secure-by-default**:
- They require a per-op `remote_allow` allowlist (deny otherwise).
- `http://` is rejected unless `allow_http = true`.

Remote normalization:
- `gen://host/path` is normalized to `https://host/path`.
- Remotes are normalized to a `.../v1/` base (e.g. `https://example.com/registry/v1/`).
- Matching is prefix-based against the normalized base.

Example:
```toml
allow = ["core/sync::pull", "core/sync::push", "core/pkg-low::publish"]

[op."core/sync::pull"]
remote_allow = ["https://registry.example.com/v1/"]
auth_token_env = "GENESIS_REGISTRY_TOKEN"
mtls_ca_pem = "./certs/registry-ca.pem"
mtls_identity_pem = "./certs/client-identity.pem"

[op."core/sync::push"]
remote_allow = ["https://registry.example.com/v1/"]
auth_token_env = "GENESIS_REGISTRY_TOKEN"
mtls_ca_pem = "./certs/registry-ca.pem"
mtls_identity_pem = "./certs/client-identity.pem"

[op."core/pkg-low::publish"]
remote_allow = ["https://registry.example.com/v1/"]
auth_token_env = "GENESIS_REGISTRY_TOKEN"
```

### `base_dir` For Non-`io/fs::*` Ops

The runner also uses `base_dir` to sandbox filesystem paths carried in payloads for some non-`io/fs::*` ops:

- `core/pkg-low::snapshot`: payload key `:pkg` (package.toml path)
- `core/pkg-low::init`: payload key `:lock` (lockfile path)
- `core/pkg-low::add`: payload key `:lock` (lockfile path)
- `core/pkg-low::lock`: payload key `:lock` (lockfile path)
- `core/pkg-low::update`: payload key `:lock` (lockfile path)
- `core/pkg-low::install`: payload key `:lock` (lockfile path)
- `core/pkg-low::verify`: payload key `:lock` (lockfile path)
- `core/pkg-low::list`: payload key `:lock` (lockfile path)
- `core/pkg-low::info`: payload key `:lock` (lockfile path)
- `core/gpk-low::export`: payload key `:out` (output `.gpk` path)
- `core/gpk-low::import`: payload key `:in` (input `.gpk` path), and optional `:set-refs` entries (`:name`, `:hash|nil`, `:policy`, optional `:expected-old`) applied through the local refs policy gate
- `core/gc-low::*`: payload keys `:lock`, `:pins`, and (optionally) `:quarantine-dir`

These payload paths must remain under `base_dir` after canonicalization, using the same rules as `io/fs::*`.

For `core/gc-low::*`, paths may refer to files/directories that do not exist yet (e.g. `.genesis/pins.toml` or `.genesis/quarantine/`). The runner validates the longest existing ancestor is within `base_dir`, rejects `..`, and then uses the resulting under-base path.

Notes on `timeout_ms`:
- Timeouts are enforced by running the capability in a background thread and waiting for a result.
- If the timeout elapses, the runner returns a sealed ERROR response with code `core/caps/timeout` and records it in the log.
- Timeouts are rejected for mutating ops such as `io/fs::write`, `io/fs::mkdir`,
  `io/fs::remove`, `io/fs::rename`, `sys/process::exec`, `sys/process::spawn`,
  `sys/process::kill`, and `sys/process::stdin-write` (policy error), to avoid
  "timed out but side-effect happened" ambiguity.
- Bridge-backed ops also honor `timeout_ms`; timeout yields deterministic `<family>/bridge-timeout`.

Bridge protocol:
- Bridge-backed ops use framed stdin/stdout payloads as defined in
  `docs/spec/HOST_BRIDGE_PROTOCOL.md`.
- Under WASI bridge profile, command spawning is replaced by deterministic configured responses:
  - per-op `wasi_bridge_response` / `wasi_bridge_response_file`, or
  - process-level `GENESIS_WASI_BRIDGE_RESPONSES` (CoreForm map `op -> response`).

## Normative Behavior

- Ops not in `allow` are denied.
- Denied ops must be recorded in the effect log with decision `:deny`.
- Allowed ops must be recorded with decision `:allow` and include a stable `:cap` term capturing the policy fields used.

## Path Resolution

When loaded from disk, relative `base_dir` paths are resolved relative to the directory containing the `caps.toml` file.
