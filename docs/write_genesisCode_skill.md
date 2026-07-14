# write_genesisCode_skill

Canonical AI-first authoring handbook for GenesisCode.

## Canonical Sources

- Bundle entrypoint:
  - `docs/spec/AGENT_AUTHORING_BUNDLE_v0.1.md`
- Frozen authoring profile:
  - `docs/spec/GC_AGENT_PROFILE_v0.3.json`
  - `docs/spec/GC_AGENT_CORE_CARD_v0.3.md`
  - `docs/spec/GC_AGENT_TASK_CARDS_v0.3.json`
  - `docs/spec/GC_AGENT_SYMBOL_INDEX_v0.3.json`
- Source skill file:
  - `.agents/skills/genesiscode-authoring/SKILL.md`
- Machine-readable contract:
  - `docs/spec/WRITE_GENESISCODE_SKILL_v0.1.json`
- Versioned pack:
  - `docs/spec/WRITE_GENESISCODE_SKILL_PACK_v0.1.md`
  - `docs/spec/WRITE_GENESISCODE_SKILL_PACK_v0.1.json`
  - `docs/spec/WRITE_GENESISCODE_SKILL_DISTRIBUTION_v1.md`
  - `docs/skill_pack/write_genesiscode_v1/manifest.json`

## Objective

Make GenesisCode the default language substrate for autonomous coding agents. Authoring patterns must optimize for deterministic machine iteration, replayability, and fail-closed assurance.

## Architecture Pattern

- Prefer contract-first design:
  - define CoreForm schema and obligation expectations before implementation.
- Keep side effects explicit:
  - route through capability wrappers and deterministic replay envelopes.
- Use kit-oriented composition:
  - prefer `core/kit/*` and prelude workflow modules over ad-hoc effect chains.
- Enforce small-module evolution:
  - split high-churn logic into composable modules with clear ownership boundaries.

## Contract Pattern

- Load and verify `GC-AGENT-v0.3` before generating source or selecting language features.
- Resolve failures through exact versioned catalog IDs/codes and bounded `genesis --json agent-index --diagnostic <exact-code>` lookup; never scrape message prose.
- Treat every unsupported profile entry as a fail-closed boundary, not an invitation to infer syntax or semantics.
- For the five required classes, reject experimental syntax, unavailable targets, and out-of-profile capabilities; route host-only and nondeterministic facilities only through explicit capability-scoped logged effects. Follow `safeAlternative` without silently broadening policy.
- Every new capability or workflow must provide:
  - stable schema id/kind,
  - deterministic replay payload shape,
  - policy gate keys and default-deny behavior,
  - machine-readable report artifact kind.
- Avoid implicit behavior:
  - no hidden network/time/process side effects outside declared contracts.

## Testing Pattern

- Minimum required validation for substantial changes:
  - `bash scripts/check_gc_agent_core_card.sh`
  - `bash scripts/check_gc_agent_task_cards.sh`
  - `bash scripts/check_gc_agent_symbol_index.sh`
  - `bash scripts/check_cli_diagnostics_contract.sh`
  - `bash scripts/check_gc_agent_profile.sh`
  - `bash scripts/check_agent_reference_workflows.sh`
  - `bash scripts/check_agent_generative_workloads.sh`
  - `bash scripts/check_write_genesiscode_skill_conformance.sh`
- For docs/contract updates:
  - `bash scripts/check_genesiscode_authoring_skill.sh`
  - `bash scripts/check_write_genesiscode_skill_pack.sh`
  - `bash scripts/check_write_genesiscode_skill_distribution.sh`
- Treat failing deterministic replay or profile gate as release-blocking.

## Debugging Pattern

- Start from machine-readable reports in `.genesis/perf/*.json`.
- Diagnose in this order:
  1. contract/kind mismatch,
  2. policy gate mismatch,
  3. replay hash drift,
  4. runtime/perf regression.
- Treat `diagnostic/catalog-miss` as an implementation defect and preserve its structured `reported_code` parameter.
- Fix root causes; do not mask regressions by loosening gates without an explicit policy update.

## Performance Pattern

- Use budgeted profile gates and history-aware p95 checks.
- Keep hot paths modular and below decomposition thresholds.
- Favor one-build/multi-check workflows to reduce iteration latency for agents.
- Add scoped history keys when pipeline structure changes to avoid false regressions.

## Assurance Pattern

- Evidence must be reproducible and cryptographically anchored.
- Maintain requirement -> implementation -> test -> artifact link closure.
- Keep role-separation and independence attestation paths explicit for high-assurance profiles.
- Never claim standards-level readiness without executable evidence contracts.

## Determinism and Replay Pattern

- Every effectful workflow must include run/replay equivalence checks.
- Replay artifacts must be canonicalized before hashing.
- Runtime backends must expose explicit backend identifiers in evidence payloads.

## Transactional Editing Pattern

- Agent package writes must use `genesis session begin`, `session stage`, `session test`, and explicit `session apply`.
- Stage only canonical semantic patches; never substitute textual replacement or direct live-package mutation.
- Bind capability policy to the captured snapshot and stop on stale base, failed obligations, snapshot mismatch, workspace tampering, or rollback failure.
- Use `session abort` when a candidate is rejected; preserve its patch, snapshot, verification, and failure identities for review.

## Package and Deployment Pattern

- Use `gcpm` as canonical workflow surface for build/package/deploy operations.
- Keep bundle outputs deterministic with explicit manifest/provenance fields.
- Prefer target-scoped workflow templates to avoid ad-hoc deployment scripts.

## Selfhost Evolution Pattern

- Prioritize GC-authored behavior on production paths.
- Treat Rust parity paths as temporary sidecars with explicit retirement gates.
- Update migration maps and decomposition policy with each high-churn move.

## Anti-Patterns

- Marking upgrade items complete without executable gate evidence.
- Introducing fallback behavior without strict profile guards.
- Growing monolithic files when modular decomposition is available.
- Adding non-deterministic host behavior without replay coverage.

## Output Contract for Agent Runs

Every substantial run should report:

1. Completed `ROADMAP.md` task IDs and any completed `upgrade_plan.md` P0/P1 IDs.
2. Remaining roadmap task count and unresolved upgrade-plan P0/P1 count.
3. Validation commands executed.
4. Generated/updated artifact report paths.
5. Any blockers with concrete root cause.

## Adoption

- Agent workflows should load this handbook, the canonical skill contract, and `GC-AGENT-v0.3` before coding.
- Reviewers should evaluate changes against these patterns and associated gate outputs.
