# GCPM JSON Schemas v0.1

This document freezes the `--json` schema IDs for all currently implemented `genesis gcpm` commands.

## Shared Envelope

All commands return:

- top-level `ok` boolean
- top-level `kind` schema ID (table below)
- top-level `data` object with deterministic fields:
  - `coreform_frontend`
  - `caps`
  - `log`
  - `value`
  - `value_format`

`gcpm doctor` additionally includes `data.doctor` with schema
`genesis/pkg-doctor-report-v0.2` (see `docs/spec/GCPM_DIAGNOSTICS_v0.1.md`).

`gcpm lock/update/publish` additionally include `data.report` workflow artifacts
(see `docs/spec/GCPM_WORKFLOW_REPORTS_v0.1.md`).

All `gcpm` commands include prompt-safe deterministic telemetry under `data.telemetry`
(see `docs/spec/GCPM_TELEMETRY_v0.1.md`).

## Command -> Kind

- `gcpm init` -> `genesis/pkg-init-v0.1`
- `gcpm new` -> `genesis/pkg-new-v0.1`
- `gcpm add` -> `genesis/pkg-add-v0.1`
- `gcpm remove` -> `genesis/pkg-remove-v0.1`
- `gcpm lock` -> `genesis/pkg-lock-v0.1`
- `gcpm update` -> `genesis/pkg-update-v0.1`
- `gcpm run <task>` -> forwards to task-target command `kind` (for built-ins: `test -> genesis/test-v0.2`, `pack -> genesis/pack-v0.2`, `typecheck -> genesis/typecheck-v0.2`)
- `gcpm test` -> `genesis/test-v0.2`
- `gcpm install` -> `genesis/pkg-install-v0.1`
- `gcpm verify` -> `genesis/pkg-verify-v0.1`
- `gcpm doctor` -> `genesis/pkg-doctor-v0.1`
- `gcpm list` -> `genesis/pkg-list-v0.1`
- `gcpm info` -> `genesis/pkg-info-v0.1`
- `gcpm abi` -> `genesis/pkg-abi-v0.1` (schema: `docs/spec/GCPM_ABI_INDEX_v0.1.md`)
- `gcpm snapshot` -> `genesis/pkg-snapshot-v0.1`
- `gcpm export` -> `genesis/pkg-export-v0.1`
- `gcpm import` -> `genesis/pkg-import-v0.1`
- `gcpm publish` -> `genesis/pkg-publish-v0.1`
- `gcpm migrate` -> `genesis/pkg-migrate-v0.1`
- `gcpm env` -> `genesis/pkg-env-v0.1`

## Determinism

- `pkg` and `gcpm` aliases MUST return identical `kind` for equivalent commands.
- Schema IDs are versioned; backward-incompatible changes require a new `kind` version.
