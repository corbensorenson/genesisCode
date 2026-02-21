> Bundle Entry: `docs/spec/GCPM_BUNDLE_v0.1.md`
> Legacy Split Doc: Prefer the bundle entrypoint for agent retrieval; this file retains detailed, topic-local semantics.

# GCPM Diagnostics Contract v0.1

This document defines the deterministic machine-readable diagnostics contract for `genesis gcpm doctor`.

## Command

- `genesis gcpm --caps <caps.toml> doctor [--lock genesis.lock] --json`
- Compatibility alias: `genesis pkg ... doctor ... --json`

## JSON Envelope

The CLI envelope follows the global schema in `docs/spec/CLI.md` with:

- `kind = "genesis/pkg-doctor-v0.1"`
- `data.doctor` present on doctor responses

## `data.doctor` Schema

`data.doctor` object:

- `schema`: fixed string `genesis/pkg-doctor-report-v0.2`
- `ok`: boolean
- `base_ok`: boolean (pre-diagnostics command success)
- `issue_count`: integer
- `exit_code`: integer
- `lock`: string path
- `caps`: string path
- `checks`: vector of check objects
- `fixes`: vector of fix objects

Check object fields:

- `id`: stable check identifier string
- `ok`: boolean
- `severity`: one of `info|error`
- `message`: deterministic message
- optional command-specific fields (for example `missing_count`)

Fix object fields:

- `id`: stable fix identifier string
- `action`: machine-action object with deterministic `op` + `args`
- `command`: deterministic suggested command/action string
- `why`: short deterministic rationale

## Determinism Requirements

- Same inputs (lock, caps, store, refs state) MUST produce byte-stable doctor diagnostics.
- Check ordering and fix ordering are stable and deterministic.
- Alias choice (`pkg` vs `gcpm`) MUST NOT change `kind` or schema.

## Current Check IDs

- `caps.parse`
- `lock.parse`
- `lock.drift`
- `lock.verify`
- `store.artifacts`
- `effects.execution`

## Current Fix IDs

- `rebuild-lock`
- `materialize-artifacts`
- `allow-required-ops`
- `generate-lock`
- `init-lock`
