# Agent Authoring Bundle v0.1

Canonical entrypoint for AI agents authoring GenesisCode projects.

Use this bundle first; open split specs only when a task requires field-level detail.

## Included Specs

- `docs/spec/GC_AGENT_CORE_CARD_v0.3.md`
- `docs/spec/GC_AGENT_CORPUS_v0.1.json`
- `docs/spec/GC_AGENT_CORPUS_v0.1.schema.json`
- `docs/spec/GC_CANONICAL_EXAMPLES_v0.1.schema.json`
- `docs/spec/GC_AGENT_TASK_BENCHMARK_v0.1.schema.json`
- `docs/spec/GC_AGENT_BENCHMARK_SCORING_v0.1.json`
- `docs/spec/GC_AGENT_BENCHMARK_SCORING_v0.1.schema.json`
- `docs/spec/GC_AGENT_BENCHMARK_SCORE_v0.1.schema.json`
- `docs/spec/GC_AGENT_HELD_OUT_EVALUATION_v0.1.json`
- `docs/spec/GC_AGENT_HELD_OUT_EVALUATION_v0.1.schema.json`
- `docs/spec/GC_AGENT_HELD_OUT_PRIVATE_PACK_v0.1.schema.json`
- `docs/spec/GC_AGENT_PROFILE_v0.3.json`
- `docs/spec/GC_AGENT_TASK_CARDS_v0.3.md`
- `docs/spec/GC_AGENT_TASK_CARDS_v0.3.json`
- `docs/spec/GC_AGENT_SYMBOL_INDEX_v0.3.json`
- `docs/spec/CLI_TOOLING_BUNDLE_v0.1.md`
- `docs/spec/GCPM_BUNDLE_v0.1.md`
- `docs/spec/HOST_RUNTIME_BUNDLE_v0.1.md`
- `docs/spec/TESTING_BUNDLE_v0.1.md`
- `docs/spec/AGENT_INDEX_v0.1.md`
- `docs/spec/AGENT_CAPABILITY_GAUNTLET_v0.1.md`
- `docs/spec/WRITE_GENESISCODE_SKILL_v0.1.md`
- `docs/spec/WRITE_GENESISCODE_SKILL_PACK_v0.1.md`
- `docs/spec/WRITE_GENESISCODE_SKILL_PACK_v0.1.json`
- `docs/spec/WRITE_GENESISCODE_SKILL_DISTRIBUTION_v1.md`
- `docs/skill_pack/write_genesiscode_v1/manifest.json`
- `docs/write_genesisCode_skill.md`
- `examples/canonical_language/v0.1/README.md`
- `examples/canonical_language/v0.1/suite.json`
- `benchmarks/agent_tasks/v0.1/suite.json`
- `scripts/lib/gc_agent_scoring.py`
- `scripts/lib/gc_agent_scoring_contract.py`

## Legacy Split Docs (must stay marked)

- `docs/spec/CLI_JSON_SCHEMAS_v0.1.md`
- `docs/spec/GCPM_JSON_SCHEMAS_v0.1.md`
- `docs/spec/HOST_BRIDGE_PROTOCOL.md`
- `docs/spec/GPU_COMPUTE_RUNTIME_PROFILE_v0.1.md`

## Agent Guidance

- Treat this bundle as the normative retrieval root for common workflows.
- Load the compact core card, then negotiate `GC-AGENT-v0.3` before generating source; profile membership describes
  syntax and semantics but never grants a host capability.
- Declare `genesis/agent-intent-v0.1` and consume `agent-plan.plan.context_cards` rather
  than loading every domain document or selecting guidance from prompt text alone.
- Validate capabilities and contracts through `genesis --json agent-index`.
- Resolve failures through bounded `genesis --json agent-index --diagnostic <exact-code>` records from the closed, content-addressed diagnostic catalog; never route on message prose.
- Learn or repair a language construct by selecting its signed pair in `GC-CANONICAL-EXAMPLES-v0.1`, executing both sides through the recorded production argv, and changing only the declared `replace-once` mutation. Never train on an invalid example without its rejection class and valid repair partner.
- Evaluate generation, completion, repair, refactor, policy minimization, replay investigation, performance repair, package migration, and deployment against `GC-AGENT-TASK-BENCHMARK-v0.1`. Treat its references as public development oracles, never as held-out evidence.
- Score a candidate with `GC-AGENT-BENCHMARK-SCORING-v0.1`. Its closed 10,000-basis-point quality result covers semantics, obligations, effects, patch minimality, deterministic resource units, and policy scope. Wall time, API cost, energy, and provider queue time are model/run facts for `genesis/agent-benchmark-run-v0.1`; they never enter the quality score.
- Make a held-out claim only against the active epoch in `GC-AGENT-HELD-OUT-v0.1`. Keep case payloads, salts, and oracles under ignored `.genesis/private/agent-evaluation`; bind every result to the epoch and commitment snapshot; use `unknown` contamination whenever training provenance is incomplete; and rotate before reuse after compromise.
- Keep authoring guidance synchronized with
  `.agents/skills/genesiscode-authoring/SKILL.md`.

## Held-Out Evaluation Custody

The public repository and documentation site may expose commitments and lifecycle metadata only. Training and retrieval may ingest every tracked file, so private prompts, inputs, oracles, salts, and payloads belong exclusively under ignored `.genesis/private/agent-evaluation/<epoch>/pack.json`, with restrictive permissions and an evaluator account outside model retrieval roots. CI never requires this private pack, and the authoring gate rejects any tracked custody path.

Each case commitment is `sha256("genesis/agent-held-out-case/v0.1\\0" || 32-byte-secret-salt || canonical-case-bytes)`. Domain separation prevents cross-protocol reuse; secret random salts prevent dictionary testing against low-entropy task details. Every result binds the exact epoch commitment-snapshot identity, not a branch or bare label.

Exactly one epoch is active and history is append-only. Provision and publish a fresh replacement before marking an epoch retired or compromised; retain old commitments, never silently rescore, and label affected results `declared-contaminated`. Scheduled disclosure requires retirement plus 30 days. A compromise-forensics disclosure requires an active replacement. Disclosure publishes payload, salt, reason, date, and the recomputable original commitment; disclosed cases become public development material permanently.

Use contamination label `uncontaminated` only when training provenance excludes every private fact and prior disclosure. Use `declared-contaminated` for known exposure and mandatory default `unknown` for missing or incomplete provenance. Never aggregate across epochs. This protocol protects secrecy and precommitment only; `GC-AGENT-BENCHMARK-SCORING-v0.1` defines model-agnostic quality, while R1.4.f defines model/run reproducibility.
