> Bundle Entry: `docs/spec/GCPM_BUNDLE_v0.1.md`
> Legacy Split Doc: Prefer the bundle entrypoint for agent retrieval; this file retains detailed, topic-local semantics.

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
`genesis/pkg-doctor-report-v0.2` (defined in this document).

`gcpm lock/update/publish` additionally include `data.report` workflow artifacts
(see `docs/spec/GCPM_WORKFLOW_REPORTS_v0.1.md`).

All `gcpm` commands include prompt-safe deterministic telemetry under `data.telemetry`
(contract defined in this document).

`gcpm env` embeds runtime backend profile contract fields in canonical CoreForm `data.value`
(`:runtime-backend-profile`, `:active-runtime-backend-profile`, `:runtime-backend-compatible`).

## Command -> Kind

- `gcpm init` -> `genesis/pkg-init-v0.1`
- `gcpm new` -> `genesis/pkg-new-v0.1`
- `gcpm add` -> `genesis/pkg-add-v0.1`
- `gcpm remove` -> `genesis/pkg-remove-v0.1`
- `gcpm lock` -> `genesis/pkg-lock-v0.1`
- `gcpm update` -> `genesis/pkg-update-v0.1`
- `gcpm build --target <web|desktop|service>` -> `genesis/pkg-build-v0.1`
- `gcpm run <task>` -> forwards to task-target command `kind`:
  - `test` -> `genesis/test-v0.2`
  - `pack|build` -> `genesis/pack-v0.2`
  - `typecheck|lint` -> `genesis/typecheck-v0.2`
  - `run|bench|contract` -> `genesis/run-v0.2`
  - `eval` -> `genesis/eval-v0.2`
  - `fmt` -> `genesis/fmt-v0.2`
  - `optimize` -> `genesis/optimize-v0.2`
- `gcpm test` -> `genesis/test-v0.2`
- `gcpm self-optimize` -> `genesis/pkg-self-optimize-v0.1`
- `gcpm profile-runtime` -> `genesis/pkg-runtime-profile-v0.1`
- `gcpm trace` -> `genesis/pkg-requirements-trace-v0.1`
- `gcpm qualify` -> `genesis/pkg-tool-qualification-v0.1`
- `gcpm assurance-pack` -> `genesis/pkg-assurance-pack-v0.1`
- `gcpm install` -> `genesis/pkg-install-v0.1`
- `gcpm verify` -> `genesis/pkg-verify-v0.1`
- `gcpm doctor` -> `genesis/pkg-doctor-v0.1`
- `gcpm list` -> `genesis/pkg-list-v0.1`
- `gcpm info` -> `genesis/pkg-info-v0.1`
- `gcpm abi` -> `genesis/pkg-abi-v0.1` (schema: this document, `GCPM ABI Contract` section)
- `gcpm snapshot` -> `genesis/pkg-snapshot-v0.1`
- `gcpm export` -> `genesis/pkg-export-v0.1`
- `gcpm import` -> `genesis/pkg-import-v0.1`
- `gcpm publish` -> `genesis/pkg-publish-v0.1`
- `gcpm migrate` -> `genesis/pkg-migrate-v0.1`
- `gcpm env` -> `genesis/pkg-env-v0.1`

## GCPM ABI Contract (`genesis/pkg-abi-v0.1`)

Normative schema for `genesis gcpm abi --pkg <package.toml>`.

Purpose:

- deterministic package introspection index for agent planning
- contract op tables, declared/inferred type+effect signatures
- required capabilities and manifest obligations

The command is pure/local and emits `kind = "genesis/pkg-abi-v0.1"`.

### CoreForm value schema

Top-level map keys:

- `:ok` (`bool`)
- `:schema` (`"genesis/pkg-abi-v0.1"`)
- `:package` (`map`)
- `:obligations` (`vector` of obligation symbols)
- `:required-caps` (`vector` of capability op symbols)
- `:module-count` (`int`)
- `:export-count` (`int`)
- `:typecheck-ok` (`bool`)
- `:typecheck-errors` (`vector` of strings)
- `:typecheck-warnings` (`vector` of strings)
- `:modules` (`vector` of per-module maps)
- `:index` (`map` from exported symbol -> export ABI entry)

Per-module ABI payload includes:

- `:path`, `:hash`, `:intent`
- `:exports`, `:declared-caps`, `:required-caps`, `:inferred-ops`
- `:unknown-ops`
- `:declared-types`
- `:typecheck-ok`, `:typecheck-errors`, `:typecheck-warnings`
- `:exports-abi`

Export ABI entry keys:

- `:name`, `:module`
- `:declared-type`, `:inferred-type`
- `:effect-signature-ops`, `:effect-signature-open`
- `:required-caps`
- `:contract-ops` (`:op`, `:type`, `:effect-signature-ops`, `:effect-signature-open`)

## Determinism

- `pkg` and `gcpm` aliases MUST return identical `kind` for equivalent commands.
- Schema IDs are versioned; backward-incompatible changes require a new `kind` version.
