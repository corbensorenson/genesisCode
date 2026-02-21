> Bundle Entry: `docs/spec/GCPM_BUNDLE_v0.1.md`
> Legacy Split Doc: Prefer the bundle entrypoint for agent retrieval; this file retains detailed, topic-local semantics.

# GCPM Environment Realization v0.1

This document defines the deterministic profile environment realization used by:

- `genesis gcpm env --profile <name> [--runtime-backend <token>] [--hydrate]`

## Goals

- Deterministically realize workspace profile configuration as immutable artifacts.
- Keep profile state machine-readable for AI agents.
- Ensure profile capability surfaces are policy-gated by requiring declared caps policy files.

## Inputs

- `genesis.workspace.toml` (default path)
- `genesis.lock` (default path)
- `profile` name (`dev` default)
- `out_dir` (`.genesis/env` default)

## Validation

`gcpm env` fails if:

- the profile does not exist in `genesis.workspace.toml`
- the resolved `caps_policy` file for the profile does not exist
- any locked dependency artifact referenced by `genesis.lock` is missing from local `.genesis/store`
- resolved runtime backend profile contract is incompatible with active CLI runtime backend profile

When `--hydrate` is set, `gcpm env` first computes missing lock-pinned commit/snapshot hashes and
fetches them through policy-gated `core/store::get` before materialization. Hydration is
deterministic (sorted hash order) and recorded in the effect log.

Relative `caps_policy` paths are resolved against the workspace descriptor directory.
`runtime_backend` tokens are canonicalized to `headless|gpu|gfx|backend` (`profile-*` aliases accepted).

## Artifact Layout

For environment hash `<env-h>`:

- `.genesis/env/<env-h>/env.gcenv`
- `.genesis/env/<env-h>/provenance.gc`
- `.genesis/env/<env-h>/workspace.toml`
- `.genesis/env/<env-h>/genesis.lock`
- `.genesis/env/<env-h>/profile.gc`
- `.genesis/env/<env-h>/members.gc`
- `.genesis/env/<env-h>/deps.gc`
- `.genesis/env/<env-h>/caps-policy.toml`
- `.genesis/env/<env-h>/toolchain.gc` (when a profile/default toolchain is configured)

`<env-h>` is BLAKE3 over canonical `env.gcenv` bytes.

## Immutability Rule

- If an env artifact path already exists with identical bytes, command is idempotent.
- If bytes differ, command fails; existing artifacts are not overwritten.

## JSON Kind

- `gcpm env` emits `kind = "genesis/pkg-env-v0.1"`.
- `data.value`/`profile.gc` include:
  - `:runtime-backend-profile`
  - `:active-runtime-backend-profile`
  - `:runtime-backend-compatible`
