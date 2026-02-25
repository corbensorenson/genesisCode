# Agent Index v0.1

This document defines the JSON contract for `genesis agent-index`.

## Purpose

Provide a single machine-oriented planning artifact for AI agents that combines:

- CLI command schema (`genesis/cli-schema-v0.1`)
- Host/prelude capability indices
- Default obligation set
- Reference workflow pointers

## Command

```bash
genesis --json agent-index
```

## Envelope

- success `kind`: `genesis/agent-index-v0.1`
- failure `kind`: `genesis/error-v0.2`

## Success Payload

`data` fields:

- `schema`: `"genesis/agent-index-v0.1"`
- `runtime_profile`: `"production"` or `"parity-harness"`
- `cli_schema`:
  - `schema`: `"genesis/cli-schema-v0.1"`
  - `command`: full recursive command schema object
- `capability_indices`:
  - `host_abi`:
    - `path`: `"docs/spec/HOST_ABI_INDEX_v0.1.json"`
    - `loaded`: bool
    - `index`: JSON object or `null`
  - `host_abi_schema`:
    - `path`: `"docs/spec/HOST_ABI_SCHEMA_INDEX_v0.1.json"`
    - `loaded`: bool
    - `index`: JSON object or `null`
  - `prelude_capabilities`:
    - `path`: `"docs/spec/PRELUDE_CAPABILITY_INDEX_v0.1.json"`
    - `loaded`: bool
    - `index`: JSON object or `null`
- `selfhost_symbol_index`:
  - `schema`: `"genesis/selfhost-symbol-ownership-index-v0.1"`
  - `path`: `"selfhost/toolchain_manifest.gc"`
  - `loaded`: bool
  - `module_count`: int
  - `symbol_count`: int
  - `required_symbol_count`: int
  - `unresolved_required_symbols`: vector<string>
  - `duplicate_symbol_owners`: vector<{symbol, module_paths[]}>
  - `symbols`: vector<{symbol, module_path, module_intent|null, required}>
- `obligation_defaults`: vector of obligation symbols
- `reference_workflows`: vector of workflow descriptors
- `missing_sources`: vector of unresolved source paths
- `docs`: canonical doc pointer map
  - includes `agent_authoring_bundle = "docs/spec/AGENT_AUTHORING_BUNDLE_v0.1.md"` as the
    normative retrieval entrypoint for common authoring workflows.
  - includes `write_genesiscode_skill_pack = "docs/spec/WRITE_GENESISCODE_SKILL_PACK_v0.1.md"`
    as the versioned machine-distribution authoring artifact.
  - includes `write_genesiscode_skill_distribution = "docs/spec/WRITE_GENESISCODE_SKILL_DISTRIBUTION_v1.md"`
    as the executable skill-distribution kit contract entrypoint.

## Determinism

- Output must be deterministic for identical repository state.
- `reference_workflows` are sorted lexicographically by workflow directory name.
- Missing optional indices are represented via `loaded=false` and `index=null` rather than hard failure.

## Agent Plan v0.1

`genesis agent-plan` consumes a structured intent contract and emits a deterministic workflow DAG
that is policy-checked before execution.

### Command

```bash
genesis --json agent-plan --intent <agent-intent.json|-> --caps <caps.toml> [--max-workflows <n>]
```

### Envelope

- success/failure `kind`: `genesis/agent-plan-v0.1`
- `ok = false` indicates planner closure failure (intent mismatch, policy denial, or missing workflow scripts)

### Intent contract (`genesis/agent-intent-v0.1`)

Expected JSON fields:

- `schema` (optional string, recommended: `genesis/agent-intent-v0.1`)
- `goal` (string, required)
- `domains` (optional vector<string>)
- `required_workflows` (optional vector<string>)
- `exclude_workflows` (optional vector<string>)
- `required_ops` (optional vector<string>)
- `max_workflows` (optional int)

### Success/failure payload

`data` fields include:

- `plan`:
  - `selected_workflows`
  - `nodes` + `edges` deterministic DAG
  - `required_ops`
  - `policy` precheck results (`ok`, `denied_ops`, optional `error`)
  - `plan_hash_blake3`
- `execution`:
  - `kind = genesis/agent-workflow-dag-v0.1`
  - deterministic step list and expected `effect_log_op`
- `lineage`:
  - `intent_hash_blake3`, `catalog_hash_blake3`, `plan_hash_blake3`
  - canonical evidence targets for replay/parity gates
- `failure_taxonomy`:
  - deterministic planner failure objects with `code`, `message`, `repair_hints`, optional `context`
- `repair_hints`: deduplicated top-level remediation hints for autonomous retry loops
