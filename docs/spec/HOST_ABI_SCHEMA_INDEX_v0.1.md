> Bundle Entry: `docs/spec/HOST_RUNTIME_BUNDLE_v0.1.md`
> Legacy Split Doc: Prefer the bundle entrypoint for agent retrieval; this file retains detailed, topic-local semantics.

# HOST ABI Schema Index v0.1

Machine-readable per-op host capability payload/response contracts live at:

- `/Users/corbensorenson/Documents/genesisCode/docs/spec/HOST_ABI_SCHEMA_INDEX_v0.1.json`

Generation + drift checks:

- regenerate: `bash scripts/update_capability_indices.sh`
- verify: `bash scripts/check_capability_indices.sh`

JSON contract:

- `kind = "genesis/host-abi-schema-index-v0.1"`
- `generated_from`: source path list used to build the schema index
- `operations`: map keyed by fully-qualified op symbol
  - each value contains:
    - `operation`
    - `payload`:
      - `type`
      - `required_fields`
      - `optional_fields`
      - `constraints`
    - `response_envelope`:
      - `success`: `value_kind`, `shape`
      - `error`: sealed error contract (`code_field`, `code_prefix`)

Notes:

- This schema index is agent-facing contract metadata layered on top of the canonical op surface in `HOST_ABI_INDEX_v0.1.json`.
- Unknown or underspecified ops still carry explicit schema entries with conservative defaults so agents can reason without heuristic guessing.
