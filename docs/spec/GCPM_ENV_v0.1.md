# GCPM Environment Realization v0.1

This document defines the deterministic profile environment realization used by:

- `genesis gcpm env --profile <name>`

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

Relative `caps_policy` paths are resolved against the workspace descriptor directory.

## Artifact Layout

For environment hash `<env-h>`:

- `.genesis/env/<env-h>/env.gcenv`
- `.genesis/env/<env-h>/provenance.gc`

`<env-h>` is BLAKE3 over canonical `env.gcenv` bytes.

## Immutability Rule

- If an env artifact path already exists with identical bytes, command is idempotent.
- If bytes differ, command fails; existing artifacts are not overwritten.

## JSON Kind

- `gcpm env` emits `kind = "genesis/pkg-env-v0.1"`.

