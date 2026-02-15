# `caps.toml` (Capability Policy) v0.2

This file defines the *deny-by-default* capability policy used by `genesis run` and by effectful obligations.

## Top-Level Keys

- `allow` (required): array of strings. Each string is a fully-qualified op symbol, e.g. `"sys/time::now"`.
- `log` (optional): table controlling effect log behavior (see below).
- `store` (optional): table controlling the artifact store used by `core/store::*` capabilities (see below).

Example:
```toml
allow = ["sys/time::now", "io/fs::read"]
```

## Store Policy (`[store]`)

Supported keys:
- `dir` (string): directory used for content-addressed artifacts for `core/store::*`.
  - If omitted, defaults to `<caps.toml directory>/.genesis/store`.

Example:
```toml
[store]
dir = "./.genesis/store"
```

## Log Policy (`[log]`)

Supported keys:
- `inline_max_bytes` (int): maximum number of bytes to inline inside `.gclog` `:resp` entries.
  - If a response exceeds this limit, the runner stores the response as a content-addressed artifact and records an artifact reference in the log.
- `store_dir` (string): directory used for content-addressed artifacts referenced by logs.
  - If omitted and `inline_max_bytes` is set, `store_dir` defaults to `<caps.toml directory>/.genesis/store`.

Example:
```toml
[log]
inline_max_bytes = 1048576
store_dir = "./.genesis/store"
```

## Per-Op Configuration

Some ops may accept a per-op policy object. This is represented as a TOML table keyed by op symbol.

Supported keys:
- `base_dir` (string): base directory sandbox for `io/fs::*` ops. Paths must remain under this directory after canonicalization.
- `create_dirs` (bool): if true, `io/fs::write` may create parent directories.
- `timeout_ms` (int): optional runner-side timeout (milliseconds). Only supported for non-mutating ops.
- `log_inline_max_bytes` (int): optional per-op override for log inlining.

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
