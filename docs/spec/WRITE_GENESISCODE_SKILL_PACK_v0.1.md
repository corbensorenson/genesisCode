# Write GenesisCode Skill Pack v0.1

Versioned distribution artifact for agent systems authoring GenesisCode.

This pack is designed for Codex/Claude-style coding agents where AI writes nearly all `.gc`
and tooling evolution code.

## Canonical Artifacts

- Contract JSON: `docs/spec/WRITE_GENESISCODE_SKILL_PACK_v0.1.json`
- Skill source: `.agents/skills/genesiscode-authoring/SKILL.md`
- Pointer/onboarding doc: `docs/write_genesisCode_skill.md`
- Bundle entrypoint: `docs/spec/AGENT_AUTHORING_BUNDLE_v0.1.md`
- Skill contract JSON: `docs/spec/WRITE_GENESISCODE_SKILL_v0.1.json`
- Schema refs: `docs/spec/CLI_JSON_SCHEMAS_v0.1.md`, `docs/spec/GCPM_JSON_SCHEMAS_v0.1.md`
- Test/profile refs: `docs/spec/TEST_EXECUTION_PROFILES_v0.1.md`
- Active backlog source: `upgrade_plan.md`

## Pack Objectives

1. Keep agent authoring deterministic and contract-first.
2. Keep language/runtime evolution selfhost-biased.
3. Keep edits machine-reviewable with explicit acceptance evidence.
4. Keep plan-driven iteration (open backlog -> implement -> verify -> mark complete).

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
- Intent: complete highest-impact unresolved `upgrade_plan.md` items end-to-end.
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
- `bash scripts/check_genesiscode_authoring_skill.sh`
- `bash scripts/check_write_genesiscode_skill_pack.sh`
- `bash scripts/check_agent_authoring_bundle.sh`

### `feature_matrix_claims`
- `bash scripts/check_feature_matrix_gap_hygiene.sh`
- `bash scripts/check_feature_matrix_evidence.sh`

### `selfhost_sidecar`
- `bash scripts/check_selfhost_artifact_fresh.sh`
- `bash scripts/check_selfhost_toolchain_review_fresh.sh`

### `fast_iteration_non_crate`
- `bash scripts/test_changed_fast.sh --base HEAD --runner auto --budget-ms 120000 --min-history 1`

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
- `scripts/check_write_genesiscode_skill_pack.sh`

The gate fails closed on contract drift, missing pack artifacts, or missing bundle/onboarding links.
