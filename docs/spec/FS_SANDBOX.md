# Filesystem Capability Sandbox v0.2

This document is **normative** for the built-in filesystem capabilities:

- `io/fs::read`
- `io/fs::write`

These capabilities are deny-by-default and must be explicitly allowed by `caps.toml`.

## Base Directory (`base_dir`)

For `io/fs::*` operations, the capability policy may specify a `base_dir` (string path).

- When loading `caps.toml` from disk, relative `base_dir` paths are resolved relative to the directory containing the `caps.toml` file.
- At runtime, the runner uses `canonicalize(base_dir)` as the sandbox root.

If `base_dir` is not provided, the runner uses the current working directory as the base directory (this is strongly discouraged for production).

## Input Path Validation

The effect payload for filesystem ops must be a map containing:

- `:path` (string): path to read/write

Validation rules:

- Any input containing `..` path components is rejected.
- Absolute input paths are accepted only if they resolve (after canonicalization rules below) to a location inside the sandbox base.

## Read (`io/fs::read`)

Read path resolution:

1. Compute `candidate = input_path` if absolute, else `candidate = base.join(input_path)`.
2. Compute `resolved = canonicalize(candidate)`.
3. Require `resolved.starts_with(base)`.

The runner reads bytes from `resolved`.

## Write (`io/fs::write`)

Write payload additionally contains:

- `:data` (bytes or string): bytes are written as-is; strings are UTF-8 bytes.

Write path resolution:

1. Compute `candidate = input_path` if absolute, else `candidate = base.join(input_path)`.
2. Let `parent = candidate.parent()` and optionally create directories if `create_dirs = true`.
3. Compute `parent_resolved = canonicalize(parent)` and require `parent_resolved.starts_with(base)`.
4. If `candidate` already exists and is a symlink, the write is rejected (defense-in-depth).
5. The runner writes bytes to `candidate`.

## Remaining TOCTOU Limitations (Explicit)

The sandbox is designed to prevent common path traversal and symlink escape attacks, but it is not a full OS sandbox:

- There is inherent time-of-check/time-of-use exposure between:
  - validating `parent_resolved` and performing the final open/write, and
  - resolving paths and performing the final open/read.
- A sufficiently privileged attacker with concurrent filesystem access to the sandbox directory may be able to race filesystem mutations.

For production hardening on hostile multi-tenant systems, run the effect runner inside an OS-level sandbox (container, VM, mandatory access control) and treat `caps.toml` as an allowlist for *semantic intent*, not as the only isolation boundary.

