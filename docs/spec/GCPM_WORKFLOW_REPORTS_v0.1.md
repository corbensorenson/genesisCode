# GCPM Workflow Reports v0.1

Deterministic AI-facing workflow summaries are emitted in `--json` under `data.report` for:

- `gcpm lock` -> `genesis/pkg-lock-report-v0.1`
- `gcpm update` -> `genesis/pkg-update-report-v0.1`
- `gcpm publish` -> `genesis/pkg-publish-report-v0.1`

## Purpose

These reports provide machine-actionable:

- what changed
- why it changed
- deterministic next-step options

## Shared Fields

All report objects include:

- `schema`
- `workflow`
- `changed`
- `why`
- `fix_options` (vector of deterministic options with `id`, `command`, `why`)

## Lock Report

- `lock`
- `lock_hash`
- `locked_count`
- `strict`

## Update Report

- `lock`
- `lock_hash`
- `updated_count`

## Publish Report

- `remote`
- `ref`
- `policy`
- `depth`
- `requested_commit`
- `published_commit`
- `expected_old`

Reports are emitted even when publish fails, so AI agents can continue with deterministic remediation planning.
