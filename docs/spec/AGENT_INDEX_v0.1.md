# Agent Index v0.1

This document defines the JSON contract for `genesis agent-index`.

## Purpose

Provide a single machine-oriented planning artifact for AI agents that combines:

- CLI command schema (`genesis/cli-schema-v0.1`)
- Host/prelude capability indices
- Frozen language-symbol index metadata and exact bounded lookup
- Closed diagnostic-catalog metadata and exact bounded lookup
- Default obligation set
- Reference workflow pointers
- Signed canonical valid/invalid language-example authority

## Command

```bash
genesis --json agent-index
genesis --json agent-index --symbol <exact-name>
genesis --json agent-index --diagnostic <exact-code>
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
- `language_symbol_index`:
  - `path`: `"docs/spec/GC_AGENT_SYMBOL_INDEX_v0.3.json"`
  - `kind`: `"genesis/gc-agent-symbol-index-v0.3"`
  - `profile_id`, `profile_identity_sha256`, `index_identity_sha256`
  - `symbol_count`: exact count of unique frozen profile symbols
  - `unsupported_behavior_count`, `unsupported_behavior_identity_sha256`
  - `unsupported_classes`: the five mandatory fail-closed profile classes
  - `lookup`: case-sensitive, normalization-free, single-result lookup contract
- `diagnostic_catalog`:
  - `path`: `"docs/spec/GC_DIAGNOSTIC_CATALOG_v0.1.json"`
  - `schema`, `version`, `identity_sha256`, and exact `diagnostic_count`
  - `lookup`: case-sensitive, normalization-free, single-result lookup contract
- `obligation_defaults`: vector of obligation symbols
- `reference_workflows`: vector of workflow descriptors
- `missing_sources`: vector of unresolved source paths
- `docs`: canonical doc pointer map
  - includes `gc_agent_profile = "docs/spec/GC_AGENT_PROFILE_v0.3.json"` as the frozen
    language, semantic, package, error, resource, compatibility, and unsupported-behavior
    profile that an authoring agent must negotiate before emitting source.
  - includes `gc_agent_core_card = "docs/spec/GC_AGENT_CORE_CARD_v0.3.md"` as the generated
    <=4,000-byte retrieval card whose symbols and examples are parser-checked against the profile.
  - includes `gc_agent_corpus = "docs/spec/GC_AGENT_CORPUS_v0.1.json"` as the closed,
    content-addressed train/dev/public-test corpus authority with explicit provenance,
    license, generator, profile, capability, test, difficulty, and oracle-exposure facts.
  - includes `gc_canonical_examples = "examples/canonical_language/v0.1/suite.json"` as the
    signed, recursively closed teaching authority for minimal valid/invalid language pairs,
    exact production argv, rejection classes, and deterministic one-site repairs.
  - includes `gc_agent_task_benchmark = "benchmarks/agent_tasks/v0.1/suite.json"` as the
    content-addressed public benchmark with nine immutable independent lineages, 27 child
    context conditions, and production-CLI reference verification.
  - includes `gc_agent_benchmark_scoring = "docs/spec/GC_AGENT_BENCHMARK_SCORING_v0.1.json"`
    as the closed six-dimension, 10,000-basis-point model-agnostic quality authority.
  - includes `gc_agent_benchmark_score_schema = "docs/spec/GC_AGENT_BENCHMARK_SCORE_v0.1.schema.json"`
    as the closed deterministic result contract; model latency, API cost, energy, and provider
    queue time are deliberately absent from quality and belong to the separate run record.
  - includes `gc_agent_benchmark_run_schema = "docs/spec/GC_AGENT_BENCHMARK_RUN_v0.1.schema.json"`
    as the recursively closed reproducibility record for immutable model/runtime identities,
    exact prompt assembly, integer decoding/retry controls, attempts, candidates, scores,
    normalized hosts, separate model metrics, and a complete content-addressed inventory.
  - includes `genesisbench_protocol = "docs/spec/GENESISBENCH_PROTOCOL_v0.1.json"`
    as the active self-hostable benchmark profile binding the frozen repository/runtime/docs
    snapshot, context and tool authority, contamination evidence, and ranked eligibility.
  - includes `genesisbench_construct_validity_policy`, `genesisbench_construct_validity_schema`,
    and `genesisbench_construct_validity_report` as the predeclared construct/nuisance authority,
    closed report contract, and executable alternative-design, negative-control, mutation,
    maintenance, statistical, saturation, and independent-audit evidence.
  - includes `genesisbench_reference_agent = "docs/spec/GENESISBENCH_REFERENCE_AGENT_v0.1.json"`,
    `genesisbench_reference_agent_ablations = "docs/spec/GENESISBENCH_REFERENCE_AGENT_ABLATIONS_v0.1.json"`,
    and `genesisbench_reference_agent_trace_schema = "docs/spec/GENESISBENCH_REFERENCE_AGENT_TRACE_v0.1.schema.json"`
    as the frozen Cold Acquisition scaffold, its eight paired ablations over nine shared
    lineages, and the complete no-hidden-work trajectory contract.
  - includes `genesisbench_front_door = "docs/spec/GENESISBENCH_FRONT_DOOR_v0.1.md"`,
    `genesisbench_open_agent = "docs/spec/GENESISBENCH_OPEN_AGENT_v0.5.json"`,
    `genesisbench_open_agent_v0_1 = "docs/spec/GENESISBENCH_OPEN_AGENT_v0.1.json"`,
    `genesisbench_open_agent_v0_2 = "docs/spec/GENESISBENCH_OPEN_AGENT_v0.2.json"`,
    `genesisbench_open_agent_v0_3 = "docs/spec/GENESISBENCH_OPEN_AGENT_v0.3.json"`,
    `genesisbench_open_agent_v0_4 = "docs/spec/GENESISBENCH_OPEN_AGENT_v0.4.json"`,
    `genesisbench_mlx_custody_schema = "docs/spec/GENESISBENCH_MLX_CUSTODY_v0.1.schema.json"`,
    `genesisbench_open_agent_campaign_schema = "docs/spec/GENESISBENCH_OPEN_AGENT_CAMPAIGN_v0.1.schema.json"`,
    `genesisbench_open_agent_campaign_report_schema = "docs/spec/GENESISBENCH_OPEN_AGENT_CAMPAIGN_REPORT_v0.1.schema.json"`,
    `genesisbench_open_agent_predeclaration_schema = "docs/spec/GENESISBENCH_OPEN_AGENT_PREDECLARATION_v0.1.schema.json"`,
    `genesisbench_open_agent_run_schema = "docs/spec/GENESISBENCH_OPEN_AGENT_RUN_v0.1.schema.json"`,
    `genesisbench_open_agent_tool_archive_schema = "docs/spec/GENESISBENCH_OPEN_AGENT_TOOL_ARCHIVE_v0.1.schema.json"`,
    `genesisbench_local_models_schema = "docs/spec/GENESISBENCH_LOCAL_MODELS_v0.1.schema.json"`,
    `genesisbench_local_model_preselection = "benchmarks/genesisbench/v0.1/local-models/preselection.json"`,
    `genesisbench_local_model_inventory = "benchmarks/genesisbench/v0.1/local-models/inventory.json"`,
    `genesisbench_local_model_custody_qwen3_4b = "benchmarks/genesisbench/v0.1/local-models/custody/qwen3-4b-4bit-v0.1.json"`,
    `genesisbench_adapters = "docs/spec/GENESISBENCH_ADAPTERS_v0.1.json"`,
    `genesisbench_adapters_schema = "docs/spec/GENESISBENCH_ADAPTERS_v0.1.schema.json"`,
    `genesisbench_adapter_schema = "docs/spec/GENESISBENCH_ADAPTER_v0.1.schema.json"`,
    `genesisbench_execution_run_schema = "docs/spec/GENESISBENCH_EXECUTION_RUN_v0.1.schema.json"`,
    and `genesisbench_bundle_manifest_schema = "docs/spec/GENESISBENCH_BUNDLE_MANIFEST_v0.1.schema.json"`
    as the canonical sixteen-command benchmark and registry surface, separate capability-minimal
    Open Agent campaign/predeclaration/run/report/tool-archive boundary with retained v0.1-v0.4 replay and v0.5 credential-free MLX custody, independently sandboxed Codex/provider processes, exact wire evidence, zero hidden retries, and hard server teardown; closed five-class Cold Acquisition transport contract,
    strict run/replay records, and deterministic submission bundle.
  - includes `genesisbench_registry = "docs/spec/GENESISBENCH_REGISTRY_v0.1.json"`,
    `genesisbench_submission_claim_schema = "docs/spec/GENESISBENCH_SUBMISSION_CLAIM_v0.1.schema.json"`,
    `genesisbench_signed_submission_schema = "docs/spec/GENESISBENCH_SIGNED_SUBMISSION_v0.1.schema.json"`,
    `genesisbench_registry_policy_schema = "docs/spec/GENESISBENCH_REGISTRY_POLICY_v0.1.schema.json"`,
    `genesisbench_registry_result_schema = "docs/spec/GENESISBENCH_REGISTRY_RESULT_v0.1.schema.json"`,
    `genesisbench_registry_event_schema = "docs/spec/GENESISBENCH_REGISTRY_EVENT_v0.1.schema.json"`,
    `genesisbench_registry_checkpoint_schema = "docs/spec/GENESISBENCH_REGISTRY_CHECKPOINT_v0.1.schema.json"`,
    and `genesisbench_leaderboard_schema = "docs/spec/GENESISBENCH_LEADERBOARD_v0.1.schema.json"`
    as the separate signed-admission and static-publication authority. Verification rederives
    every result from retained bundle bytes, every event prefix is operator-signed, and rank is
    lexicographic within one exact cohort without cost or latency as a tie-breaker.
  - includes `genesisbench_analysis_plan = "docs/spec/GENESISBENCH_ANALYSIS_PLAN_v0.1.json"`,
    `genesisbench_observations_schema = "docs/spec/GENESISBENCH_OBSERVATIONS_v0.1.schema.json"`,
    and `genesisbench_analysis_report_schema = "docs/spec/GENESISBENCH_ANALYSIS_REPORT_v0.1.schema.json"`
    as the predeclared lineage-clustered statistical contract and its closed evidence surfaces.
  - includes `genesisbench_baseline_protocol`, `genesisbench_baseline_predeclaration_schema`,
    `genesisbench_baseline_evidence_schema`, `genesisbench_baseline_publication_schema`,
    `genesisbench_benchmark_card`, and `genesisbench_failure_taxonomy` as the locked
    real-model study, complete attempt-custody, deterministic publication, and non-promotable
    synthetic-conformance authority.
  - includes `gc_agent_model_runner_effect = "docs/spec/GC_AGENT_MODEL_RUNNER_EFFECT_v0.1.json"`
    as the explicit deny-by-default local benchmark integration over the pinned
    `host/plugin::command` effect and replayable `.gclog` transcript.
  - includes `gc_agent_held_out_evaluation = "docs/spec/GC_AGENT_HELD_OUT_EVALUATION_v0.1.json"`
    as the public salted-commitment ledger. Its active epoch carries 90 independently committed,
    balanced lineages and exposes lifecycle, freshness, custody, chronology, and overlay facts,
    never private cases, salts, prompts, inputs, or oracles.
  - includes `genesisbench_temporal_epoch_audit = "docs/program/GENESISBENCH_TEMPORAL_EPOCH_AUDIT_v0.1.json"`
    as the hash-only record that all active private commitments and metadata bindings were opened
    under local custody without publishing private material.
  - includes `gc_capability_lease_protocol = "docs/spec/GC_CAPABILITY_LEASE_PROTOCOL_v0.1.json"`
    as the maintained, general-purpose deny-by-default lease protocol introduced by the active
    temporal overlay and retained after its benchmark ranking weight retires.
  - includes `gc_agent_task_cards = "docs/spec/GC_AGENT_TASK_CARDS_v0.3.json"` as the
    embedded seven-card registry used by deterministic intent selection.
  - includes `gc_agent_symbol_index = "docs/spec/GC_AGENT_SYMBOL_INDEX_v0.3.json"` as the
    content-addressed exact-lookup authority for signatures, effects, capabilities,
    contracts, examples, diagnostics, deprecations, and source links.
  - includes `diagnostic_catalog = "docs/spec/GC_DIAGNOSTIC_CATALOG_v0.1.json"` as the
    closed, content-addressed authority for versioned diagnostic routes and safe repair metadata.
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
- The complete language-symbol array is not embedded in the unfiltered planning response.
  `--symbol` performs an exact case-sensitive lookup and returns at most one self-contained
  `genesis/agent-symbol-v0.3` record; unknown, padded, or case-drifted names fail closed.
- The complete diagnostic array is not embedded in the unfiltered planning response.
  `--diagnostic` returns at most one self-contained `genesis/diagnostic-v0.1` record;
  unknown, padded, or case-drifted codes fail closed.

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
  - `context_cards`: selected capability, package, patch, replay, testing, deployment,
    and/or troubleshooting cards with reasons, complete content, per-card source hashes,
    aggregate token upper bound, registry identity, and selection identity. This field is
    included in `plan_hash_blake3`.
- `execution`:
  - `kind = genesis/agent-workflow-dag-v0.1`
  - deterministic step list and expected `effect_log_op`
- `lineage`:
  - `intent_hash_blake3`, `catalog_hash_blake3`, `plan_hash_blake3`
  - canonical evidence targets for replay/parity gates
- `failure_taxonomy`:
  - deterministic planner failure objects with `code`, `message`, `repair_hints`, optional `context`
- `repair_hints`: deduplicated top-level remediation hints for autonomous retry loops
