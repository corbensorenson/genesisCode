# GCPM Telemetry v0.1

`genesis gcpm --json` emits deterministic, prompt-safe telemetry under `data.telemetry`.

## Schema

- `schema`: `genesis/pkg-telemetry-v0.1`
- `command`: stable command id (for example `pkg-lock`)
- `ok`: command success boolean
- `exit_code`: numeric exit code
- `effect_log_hash`: BLAKE3 hex hash of canonical effect log bytes
- `value_hash`: deterministic value hash
- `effect_entries`: effect log entry count
- `value_kind`: coarse kind label
- optional `changed`: propagated from `data.report.changed`
- optional `doctor_issue_count`: propagated from `data.doctor.issue_count`

## Prompt-Safe Rules

- Telemetry MUST omit filesystem paths, payload bodies, and secrets.
- Telemetry MUST only expose deterministic summary/hash/count fields.
- Telemetry MUST remain stable for identical inputs and policy state.
