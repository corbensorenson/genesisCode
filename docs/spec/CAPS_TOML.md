# `caps.toml` (Capability Policy) v0.2

This file defines the *deny-by-default* capability policy used by `genesis run` and by effectful obligations.

## Top-Level Keys

- `allow` (required): array of strings. Each string is a fully-qualified op symbol, e.g. `"sys/time::now"`.

Example:
```toml
allow = ["sys/time::now", "io/fs::read"]
```

## Per-Op Configuration

Some ops may accept a per-op policy object. This is represented as a TOML table keyed by op symbol.

Supported keys:
- `base_dir` (string): base directory sandbox for `io/fs::*` ops. Paths must remain under this directory after canonicalization.
- `create_dirs` (bool): if true, `io/fs::write` may create parent directories.

Example:
```toml
allow = ["io/fs::read", "io/fs::write"]

[op."io/fs::read"]
base_dir = "./sandbox"

[op."io/fs::write"]
base_dir = "./sandbox"
create_dirs = true
```

## Normative Behavior

- Ops not in `allow` are denied.
- Denied ops must be recorded in the effect log with decision `:deny`.
- Allowed ops must be recorded with decision `:allow` and include a stable `:cap` term capturing the policy fields used.

## Path Resolution

When loaded from disk, relative `base_dir` paths are resolved relative to the directory containing the `caps.toml` file.
