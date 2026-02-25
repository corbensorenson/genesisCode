> Bundle Entry: `docs/spec/GCPM_BUNDLE_v0.1.md`
> Legacy Split Doc: Prefer the bundle entrypoint for agent retrieval; this file retains detailed, topic-local semantics.

# GCPM Workflow Reports v0.1

Deterministic AI-facing workflow summaries are emitted in `--json` under `data.report` for:

- `gcpm add` -> `genesis/pkg-add-report-v0.1`
- `gcpm remove` -> `genesis/pkg-remove-report-v0.1`
- `gcpm lock` -> `genesis/pkg-lock-report-v0.1`
- `gcpm update` -> `genesis/pkg-update-report-v0.1`
- `gcpm run` -> `genesis/pkg-run-report-v0.1`
- `gcpm build` -> `genesis/pkg-build-report-v0.1`
- `gcpm install` -> `genesis/pkg-install-report-v0.1`
- `gcpm verify` -> `genesis/pkg-verify-report-v0.1`
- `gcpm doctor` -> `genesis/pkg-doctor-ai-report-v0.1`
- `gcpm env` -> `genesis/pkg-env-report-v0.1`
- `gcpm publish` -> `genesis/pkg-publish-report-v0.1`
- `gcpm bridge` -> `genesis/pkg-bridge-report-v0.1`
- `gcpm self-optimize` -> `genesis/pkg-self-optimize-report-v0.1`

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

## Bridge Report

- `ecosystem`
- `name`
- `version`
- `source`
- `source_hash`
- `commit`
- `snapshot`
- `provenance_root`
- `conversion_evidence`
- `attestation`
- `lock`
- `dep_name`
- `registry`

## Operational Expansion

The report surface now covers dependency mutation (`add/remove`), execution/build (`run/build`), environment realization (`env`), and verification/remediation loops (`install/verify/doctor`). This keeps agents on deterministic command-level remediation paths instead of prompt-level heuristics.
