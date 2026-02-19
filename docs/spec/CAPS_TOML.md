# `caps.toml` (Capability Policy) v0.2

This file defines the *deny-by-default* capability policy used by `genesis run` and by effectful obligations.

## Top-Level Keys

- `allow` (required): array of strings. Each string is a fully-qualified op symbol, e.g. `"sys/time::now"`.
- `log` (optional): table controlling effect log behavior (see below).
- `store` (optional): table controlling the artifact store used by `core/store::*` capabilities (see below).
- `refs` (optional): table controlling the local refs database used by `core/refs::*` capabilities (see below).

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
- `create_dirs` (bool): if true, `io/fs::write` may create parent directories.
- `timeout_ms` (int): optional runner-side timeout (milliseconds). Only supported for non-mutating ops.
- `log_inline_max_bytes` (int): optional per-op override for log inlining.
- `max_bytes` (int): optional per-op byte budget for payload-heavy operations.
  - `core/store::put`: maximum artifact byte size accepted for each put request.
  - `core/store::get` and `io/fs::read`: maximum bytes allowed in the fetched/read payload.
- `remote_allow` (array of strings): allowlist of remote base URL prefixes for `core/sync::*` and `core/pkg::publish` (see below).
- `allow_http` (bool): if true, `http://` remotes are permitted for `core/sync::*` and `core/pkg::publish` (default is false).
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
```

## Sync/Publish Remotes (`core/sync::*`, `core/pkg::publish`)

`core/sync::pull`, `core/sync::push`, and `core/pkg::publish` are **secure-by-default**:
- They require a per-op `remote_allow` allowlist (deny otherwise).
- `http://` is rejected unless `allow_http = true`.

Remote normalization:
- `gen://host/path` is normalized to `https://host/path`.
- Remotes are normalized to a `.../v1/` base (e.g. `https://example.com/registry/v1/`).
- Matching is prefix-based against the normalized base.

Example:
```toml
allow = ["core/sync::pull", "core/sync::push", "core/pkg::publish"]

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

[op."core/pkg::publish"]
remote_allow = ["https://registry.example.com/v1/"]
auth_token_env = "GENESIS_REGISTRY_TOKEN"
```

### `base_dir` For Non-`io/fs::*` Ops

The runner also uses `base_dir` to sandbox filesystem paths carried in payloads for some non-`io/fs::*` ops:

- `core/pkg::snapshot`: payload key `:pkg` (package.toml path)
- `core/pkg::init`: payload key `:lock` (lockfile path)
- `core/pkg::add`: payload key `:lock` (lockfile path)
- `core/pkg::lock`: payload key `:lock` (lockfile path)
- `core/pkg::update`: payload key `:lock` (lockfile path)
- `core/pkg::install`: payload key `:lock` (lockfile path)
- `core/pkg::verify`: payload key `:lock` (lockfile path)
- `core/pkg::list`: payload key `:lock` (lockfile path)
- `core/pkg::info`: payload key `:lock` (lockfile path)
- `core/gpk::export`: payload key `:out` (output `.gpk` path)
- `core/gpk::import`: payload key `:in` (input `.gpk` path), and optional `:set-refs` entries (`:name`, `:hash|nil`, `:policy`, optional `:expected-old`) applied through the local refs policy gate
- `core/gc::*`: payload keys `:lock`, `:pins`, and (optionally) `:quarantine-dir`

These payload paths must remain under `base_dir` after canonicalization, using the same rules as `io/fs::*`.

For `core/gc::*`, paths may refer to files/directories that do not exist yet (e.g. `.genesis/pins.toml` or `.genesis/quarantine/`). The runner validates the longest existing ancestor is within `base_dir`, rejects `..`, and then uses the resulting under-base path.

Notes on `timeout_ms`:
- Timeouts are enforced by running the capability in a background thread and waiting for a result.
- If the timeout elapses, the runner returns a sealed ERROR response with code `core/caps/timeout` and records it in the log.
- Timeouts are rejected for mutating ops like `io/fs::write` (policy error), to avoid "timed out but side-effect happened" ambiguity.

## Normative Behavior

- Ops not in `allow` are denied.
- Denied ops must be recorded in the effect log with decision `:deny`.
- Allowed ops must be recorded with decision `:allow` and include a stable `:cap` term capturing the policy fields used.

## Path Resolution

When loaded from disk, relative `base_dir` paths are resolved relative to the directory containing the `caps.toml` file.
