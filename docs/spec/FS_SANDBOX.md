# Filesystem Capability Sandbox v0.2

This document is **normative** for the built-in filesystem capabilities:

- `io/fs::stat`
- `io/fs::list`
- `io/fs::mkdir`
- `io/fs::remove`
- `io/fs::rename`
- `io/fs::read`
- `io/fs::write`

These capabilities are deny-by-default and must be explicitly allowed by `caps.toml`.

## Base Directory (`base_dir`)

For `io/fs::*` operations, the capability policy may specify a `base_dir` (string path).

- When loading `caps.toml` from disk, relative `base_dir` paths are resolved relative to the directory containing the `caps.toml` file.
- At runtime, the runner uses `canonicalize(base_dir)` as the sandbox root.

If `base_dir` is not provided, the runner uses the current working directory as the base directory (this is strongly discouraged for production).

## Input Path Validation

Filesystem effect payloads are maps with op-specific required fields:

- `io/fs::{stat,list,mkdir,remove,read,write}`:
  - `:path` (string)
- `io/fs::rename`:
  - `:from` (string)
  - `:to` (string)

Validation rules:

- Input is valid UTF-8 normalized to Unicode 17 NFC.
- `.` alone names the sandbox base. Every other input is a non-empty, base-relative sequence of
  non-empty components separated by `/` on every host.
- Absolute paths, drive prefixes, backslashes, empty components, `.`, and `..` components are
  rejected before filesystem access.
- Path identity is case-sensitive and locale-independent. The runtime never folds requested case,
  even when the host filesystem does.

These language-facing rules are frozen by `docs/spec/TEXT_PATH_PROFILE_v0.1.md`. Absolute
`base_dir` values remain policy configuration and never enter a language request or response.

## Read (`io/fs::read`)

Read path resolution:

1. Compute `candidate = base.join(input_path)` after portable path validation.
2. Compute `resolved = canonicalize(candidate)`.
3. Require `resolved.starts_with(base)`.

The runner reads bytes from `resolved`.

## Write (`io/fs::write`)

Write payload additionally contains:

- `:data` (bytes or string): bytes are written as-is; strings are UTF-8 bytes.

Write path resolution:

1. Compute `candidate = base.join(input_path)` after portable path validation.
2. Let `parent = candidate.parent()` and optionally create directories if `create_dirs = true`.
3. Compute `parent_resolved = canonicalize(parent)` and require `parent_resolved.starts_with(base)`.
4. If `candidate` already exists and is a symlink, the write is rejected (defense-in-depth).
5. The runner writes bytes to `candidate`.

## Stat (`io/fs::stat`)

Stat path resolution uses the same sandbox rules as read/write, but allows missing targets.

Response envelope (data map):
- `:path` (string, path relative to `base_dir` when possible)
- `:exists` (bool)
- `:kind` (`file|dir|symlink|other|missing`)
- `:len-bytes` (int)
- `:readonly` (bool)

## List (`io/fs::list`)

List path resolution follows read-path sandbox checks and then reads directory entries.

Response envelope:
- vector of entry maps, deterministically sorted by canonical term order
- each entry map contains `:name`, `:path`, `:kind`, `:len-bytes`

Names and paths are strict UTF-8 normalized to NFC and use `/`. A non-UTF-8 entry returns trusted
sealed `core/path-encoding-error`. If distinct host names normalize to the same response identity,
the operation returns trusted sealed `core/path-collision-error`; no lossy replacement or silent
merge is allowed.

## Mkdir (`io/fs::mkdir`)

Payload fields:
- `:path` (string)
- optional `:parents` (bool, default `true`)

When `:parents` is true, parent directories are created recursively.

## Remove (`io/fs::remove`)

Payload fields:
- `:path` (string)
- optional `:recursive` (bool, default `false`)

Behavior:
- files/symlinks are removed with file semantics
- directories require `:recursive true` for recursive removal
- missing paths are treated as deterministic no-op success

## Rename (`io/fs::rename`)

Payload fields:
- `:from` (string)
- `:to` (string)
- optional `:overwrite` (bool, default `false`)

Behavior:
- both paths are sandboxed under `base_dir`
- if `create_dirs = true` in policy, destination parent directories may be created
- when `:overwrite` is false and destination exists, operation fails with policy error

## Remaining TOCTOU Limitations (Explicit)

The sandbox is designed to prevent common path traversal and symlink escape attacks, but it is not a full OS sandbox:

- There is inherent time-of-check/time-of-use exposure between:
  - validating `parent_resolved` and performing the final open/write, and
  - resolving paths and performing the final open/read.
- A sufficiently privileged attacker with concurrent filesystem access to the sandbox directory may be able to race filesystem mutations.

For production hardening on hostile multi-tenant systems, run the effect runner inside an OS-level sandbox (container, VM, mandatory access control) and treat `caps.toml` as an allowlist for *semantic intent*, not as the only isolation boundary.
