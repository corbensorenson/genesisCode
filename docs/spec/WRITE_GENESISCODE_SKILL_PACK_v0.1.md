# Write GenesisCode Skill Pack v0.1

Versioned distribution artifact for agent systems authoring GenesisCode.

This pack is designed for Codex/Claude-style coding agents where AI writes nearly all `.gc`
and tooling evolution code.

## Canonical Artifacts

- Contract JSON: `docs/spec/WRITE_GENESISCODE_SKILL_PACK_v0.1.json`
- Skill source: `.agents/skills/genesiscode-authoring/SKILL.md`
- Pointer/onboarding doc: `docs/write_genesisCode_skill.md`
- Bundle entrypoint: `docs/spec/AGENT_AUTHORING_BUNDLE_v0.1.md`
- Frozen training profile: `docs/spec/GC_AGENT_PROFILE_v0.3.json`
- Generated compact core card: `docs/spec/GC_AGENT_CORE_CARD_v0.3.md`
- Intent-selected task-card registry: `docs/spec/GC_AGENT_TASK_CARDS_v0.3.json`
- Exact language-symbol registry: `docs/spec/GC_AGENT_SYMBOL_INDEX_v0.3.json`
- Skill contract JSON: `docs/spec/WRITE_GENESISCODE_SKILL_v0.1.json`
- Executable conformance spec: `docs/spec/WRITE_GENESISCODE_SKILL_CONFORMANCE_v0.1.md`
- Distribution kit spec: `docs/spec/WRITE_GENESISCODE_SKILL_DISTRIBUTION_v1.md`
- Distribution manifest: `docs/skill_pack/write_genesiscode_v1/manifest.json`
- Schema refs: `docs/spec/CLI_JSON_SCHEMAS_v0.1.md`, `docs/spec/GCPM_JSON_SCHEMAS_v0.1.md`
- Test/profile refs: `docs/spec/TEST_EXECUTION_PROFILES_v0.1.md`
- Strategic execution source: `ROADMAP.md`
- Unresolved P0/P1 compatibility queue: `upgrade_plan.md`

## Pack Objectives

1. Keep agent authoring deterministic and contract-first.
2. Keep language/runtime evolution selfhost-biased.
3. Keep edits machine-reviewable with explicit acceptance evidence.
4. Keep plan-driven iteration (open backlog -> implement -> verify -> mark complete).
5. Freeze authoring assumptions by negotiating `GC-AGENT-v0.3` before source generation.
6. Enforce all five unsupported classes and their safe alternatives without prompt- or index-derived authority.

## Distribution Layout (Normative)

- `skill_file`
  - canonical skill text agents should load first for GenesisCode code changes.
- `contract`
  - machine-verifiable drift controls (required sections/spec refs/contract ids).
- `prompt_templates`
  - structured prompts for common work loops.
- `acceptance_profiles`
  - deterministic command bundles keyed by change class.
- `anti_patterns`
  - explicit invalid behaviors to reject automatically.

## Prompt Templates

### `backlog_slice`
- Intent: complete the highest-impact ready `ROADMAP.md` task end-to-end, except unresolved P0/P1 compatibility work remains sourced from `upgrade_plan.md`.
- Required output:
  - explicit task list
  - changed files
  - command evidence and pass/fail
  - updated remaining checklist count.

### `capability_addition`
- Intent: add/expand capability wrappers with policy + replay + docs.
- Required output:
  - wrapper contract shape
  - policy keys
  - replay determinism proof path
  - updated capability indices and drift gates.

### `selfhost_cutover`
- Intent: migrate semantics from Rust boundary into `.gc` selfhost modules.
- Required output:
  - removed fallback/deprecated path list
  - strict selfhost gate evidence
  - bootstrap/archive safety checks.

## Acceptance Profiles

### `docs_only`
- `bash scripts/check_doc_hygiene.sh`
- `bash scripts/check_planning_docs_fresh.sh`

### `authoring_contract`
- `bash scripts/check_gc_agent_core_card.sh`
- `bash scripts/check_gc_agent_task_cards.sh`
- `bash scripts/check_gc_agent_symbol_index.sh`
- `bash scripts/check_gc_agent_profile.sh`
- `bash scripts/check_genesiscode_authoring_skill.sh`
- `bash scripts/check_write_genesiscode_skill_pack.sh`
- `bash scripts/check_write_genesiscode_skill_distribution.sh`
- `bash scripts/check_agent_authoring_bundle.sh`

### `feature_matrix_claims`
- `bash scripts/check_feature_matrix_gap_hygiene.sh`
- `bash scripts/check_capability_evidence_ledger.sh`

### `skill_conformance_executable`
- `bash scripts/check_agent_reference_workflows.sh`
- `bash scripts/check_agent_generative_workloads.sh`
- `bash scripts/check_write_genesiscode_skill_conformance.sh`
- `GENESIS_WRITE_SKILL_DIST_VERIFY_RUNTIME=1 bash scripts/check_write_genesiscode_skill_distribution.sh`

### `selfhost_sidecar`
- `bash scripts/check_selfhost_artifact_fresh.sh`
- `bash scripts/check_selfhost_toolchain_review_fresh.sh`

### `fast_iteration_non_crate`
- `bash scripts/test_changed_fast.sh --base HEAD --runner auto --budget-ms 120000 --min-history 1`

## Distribution Kit v1

Canonical executable distribution kit root:

- `docs/skill_pack/write_genesiscode_v1/manifest.json`

Normative content:

- prompt templates under `docs/skill_pack/write_genesiscode_v1/prompts/`
- runnable domain recipes under `docs/skill_pack/write_genesiscode_v1/recipes/`
- project templates under `docs/skill_pack/write_genesiscode_v1/templates/`
- deterministic distribution verifier:
  `scripts/check_write_genesiscode_skill_distribution.sh`
- gpu/xr productization verifier:
  `scripts/check_gpu_xr_productization_kits.sh`

## Benchmark Suite

Required executable benchmark workloads for AI-authored GenesisCode quality:

- Agent task matrix: `benchmarks/agent_tasks/v0.1/suite.json`
- Model-agnostic scoring authority: `docs/spec/GC_AGENT_BENCHMARK_SCORING_v0.1.json`
- Closed score-result contract: `docs/spec/GC_AGENT_BENCHMARK_SCORE_v0.1.schema.json`
- Reproducible run contract: `docs/spec/GC_AGENT_BENCHMARK_RUN_v0.1.schema.json`
- Local benchmark model effect: `docs/spec/GC_AGENT_MODEL_RUNNER_EFFECT_v0.1.json`
- Held-out commitment authority: `docs/spec/GC_AGENT_HELD_OUT_EVALUATION_v0.1.json` (public commitments only; private custody material is forbidden from the skill distribution)
- Service workflow: `examples/agent_service_workflow/workflow.sh`
- Game-loop workflow: `examples/agent_long_running_gfx_loop_workflow/workflow.sh`
- GPU compute workflow: `examples/agent_gpu_compute_workflow/workflow.sh`
- GPU non-graphics workflow: `examples/agent_compute_workflow/workflow.sh`
- XR runtime workflow: `examples/agent_xr_runtime_workflow/workflow.sh`
- XR browser conformance workflow: `scripts/check_webxr_browser_conformance.sh`
- Package workflow: `examples/agent_multi_package_publish_workflow/workflow.sh`
- Mutation workload suite: `scripts/check_agent_generative_workloads.sh`

## Scoring Rubric

Candidate quality is scored by `scripts/lib/gc_agent_scoring.py` on a 10,000-basis-point scale across semantics, obligations, effects, patch minimality, deterministic resource use, and policy scope. Validity fails closed when required semantic, obligation, policy, or editable-scope facts fail. Wall time, API cost, energy, and provider queue time are separate `genesis/agent-benchmark-run-v0.1` facts and never alter quality.

Every reported benchmark invocation must also pass `python3 scripts/lib/gc_agent_benchmark_run.py --check`. The record binds immutable model, weights, tokenizer, runtime, exact prompt/card/context assembly, integer decoding and retry controls, all attempts and candidate artifacts, the canonical score, normalized host facts, and a complete artifact inventory. Local model execution is permitted only through the digest-pinned `genesis.agent-model-runner.v0.1` / `infer` effect; preserve request, response, transcript, and `.gclog`, and require replay without model reinvocation.

The skill-conformance score below measures whether this authoring pack covers its required workflows; it is not a candidate benchmark score.

- Conformance gate: `scripts/check_write_genesiscode_skill_conformance.sh`
- Report kind: `genesis/write-genesiscode-skill-conformance-v0.1`
- Default threshold: `100/100` (`GENESIS_WRITE_SKILL_CONFORMANCE_MIN_SCORE`)
- Categories (20 points each):
  - service workflow determinism + domain coverage
  - game loop determinism + graphics coverage
  - GPU compute determinism + compute coverage
  - package workflow determinism + package/sync coverage
  - generative mutation suite stability (min case count + no parity/history-min failures)

## Failure-Mode Playbooks

- Missing workflow/category:
  - restore failing workflow script under `examples/agent_*_workflow/`
  - rerun `scripts/check_agent_reference_workflows.sh`.
- Replay mismatch:
  - inspect workflow `run/replay` output deltas; reject nondeterministic effects first.
- GPU backend contract failure:
  - rerun strict device lane:
    `GENESIS_AGENT_GPU_REQUIRE_DEVICE=1 bash scripts/check_agent_reference_workflows.sh`.
- Generative regression or insufficient history:
  - refresh baseline history seeds under `policies/perf/`
  - rerun `scripts/check_agent_generative_workloads.sh` with explicit regression params.

## Anti-Patterns (Reject)

- Claiming feature parity without capability-evidence ledger updates.
- Marking plan items done without command evidence.
- Leaving deprecated runtime paths active after selfhost replacement.
- Emitting non-deterministic host effects without replay coverage.
- Expanding monolithic files when module boundaries are obvious.

## Agent Output Contract

Every substantial turn should include:

1. Completed IDs.
2. Remaining IDs count.
3. Validation commands run.
4. Any blockers + root cause.
5. Next highest-impact slices.

## Conformance

Gate:
- `scripts/check_gc_agent_core_card.sh`
- `scripts/check_gc_agent_task_cards.sh`
- `scripts/check_gc_agent_symbol_index.sh`
- `scripts/check_gc_agent_profile.sh`
- `scripts/check_write_genesiscode_skill_pack.sh`
- `scripts/check_write_genesiscode_skill_guide.sh`
- `scripts/check_write_genesiscode_skill_distribution.sh`
- `scripts/check_write_genesiscode_skill_conformance.sh`
- `scripts/check_gpu_xr_productization_kits.sh`

The gate fails closed on contract drift, missing pack artifacts, or missing bundle/onboarding links.
