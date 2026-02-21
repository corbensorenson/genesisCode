# GCPM Workspace Descriptor v0.1

Genesis workspace roots are described by `genesis.workspace.toml`.

## Schema

- `version = 1`
- `workspace = "<name>"`
- `[[members]]` with:
  - `name`
  - `path`
  - optional `role`
- `[defaults]`:
  - optional `registry`
  - optional `policy`
  - optional `toolchain` (selfhost toolchain artifact pin; relative to workspace file unless absolute)
- `[profiles.<name>]`:
  - optional `caps_policy`
  - optional `registry`
  - optional `policy`
  - optional `toolchain`
- `[tasks.<name>]`:
  - `cmd`
  - optional `file`
  - optional `pkg`
  - optional `args = ["..."]`

### Contract Task Hook (AI-First)

`cmd = "contract"` is a hash-pinned custom task hook for agent-defined workflows:

- requires `file` (CoreForm effect program path)
- requires `args` to include `--contract-h <hex64>`
- optional args:
  - `--caps <path>`
  - `--log <path>`
  - `--engine <selfhost|rust>`

Before execution, `gcpm run` verifies `blake3(file-bytes) == --contract-h`.
Mismatch fails closed and task execution is refused.

## Determinism

- Canonical writer must produce stable output ordering.
- Member names and paths must be unique.
- Production frontend flows may resolve `defaults.toolchain` as the workspace-pinned selfhost
  artifact identity when `--selfhost-artifact`/`GENESIS_SELFHOST_TOOLCHAIN_ARTIFACT` are not set.

## Commands

- `genesis gcpm new` creates `genesis.workspace.toml` + `genesis.lock`.
- `genesis gcpm migrate` creates workspace + lock from `package.toml`.
- `genesis gcpm remove <name>` deterministically removes requirement + locked entry from lock.
- `genesis gcpm run <task>` resolves and executes workspace task command data (built-ins: `test`, `pack`, `build`, `typecheck`, `lint`, `run`, `bench`, hash-pinned `contract`, `eval`, `fmt`, `optimize`).
- `genesis gcpm env --profile <name> [--hydrate]` materializes deterministic profile environment artifacts (see `docs/spec/GCPM_ENV_v0.1.md`).
  - `--hydrate` pulls missing lock-pinned artifacts through policy-gated `core/store::get` before writing env artifacts.
