# `package.toml` (Package Manifest) v0.2

This file defines a package, its modules, dependencies, and required obligations.

## Required Keys

- `name` (string)
- `version` (string)
- `modules` (array of tables): `[{ path = "...", hash = "..." }, ...]`
- `dependencies` (array of tables): `[{ name = "...", path = "...", hash = "..." }, ...]`
- `obligations` (array of strings): obligation IDs to run

## Optional Keys

- `tests` (array of strings): suite symbols to execute as unit tests
- `caps_policy` (string): path to a `caps.toml` relative to the manifest directory
- `limits` (table): evaluation limits enforced for package evaluation and tests

`limits` keys:
- `step_limit` (integer, optional): kernel evaluation step limit for package evaluation/tests
  - If omitted, the v0.2 toolchain default is used.
- `allow_unlimited` (bool, default `false`): if `true`, permits disabling the step limit via `genesis test --no-step-limit`.

## Module Table

Each entry:
- `path` (string): module file path (relative to the manifest directory, using `/` separators; must not contain `.` or `..`)
- `hash` (string): BLAKE3 hex of the module *canonical printed bytes* with the `GCv0.2` tag

`genesis pack --pkg package.toml` computes and writes module hashes.

## Dependency Table

Each entry:
- `name` (string)
- `path` (string): local path to dependency package directory (relative, using `/` separators; must not contain `.` or `..`)
- `hash` (string): BLAKE3 hex of the dependency package artifact hash (as produced by `genesis pack`)

## Normative Behavior

- `genesis test` must verify that each module’s current hash matches the pinned `hash` field.
- Dependencies must be hash-checked before use (local path deps are allowed but must match pinned hashes).
- Package acceptance is granted only if all listed `obligations` succeed.
- Package evaluation limits are enforced for `genesis test` and `genesis apply-patch`:
  - if `limits.allow_unlimited = false` (default), `--no-step-limit` must be rejected as a manifest policy error
  - the effective step limit is the minimum of the CLI request (if any) and `limits.step_limit` (or the toolchain default when omitted)
