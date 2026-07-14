# Agent Onboarding Path v0.1

Last updated: 2026-02-22

Purpose: provide a minimal, stable retrieval path for AI coding agents.
This file is the single agent-first onboarding spine.

## Canonical Read Order (Required)

1. `docs/INDEX.md`
2. `docs/spec/GC_AGENT_CORE_CARD_v0.3.md`
3. `docs/spec/GC_AGENT_PROFILE_v0.3.json`
4. `docs/spec/GC_AGENT_TASK_CARDS_v0.3.md` selected through declared task intent
5. `docs/spec/GC_AGENT_SYMBOL_INDEX_v0.3.json` through exact `agent-index --symbol` lookup
6. `docs/spec/GC_DIAGNOSTIC_CATALOG_v0.1.json` through exact `agent-index --diagnostic` lookup
7. `docs/spec/CLI_TOOLING_BUNDLE_v0.1.md`
8. `docs/spec/GCPM_BUNDLE_v0.1.md`
9. `docs/spec/HOST_RUNTIME_BUNDLE_v0.1.md`
10. `docs/spec/TESTING_BUNDLE_v0.1.md`
11. `docs/spec/AGENT_AUTHORING_BUNDLE_v0.1.md`
12. `docs/spec/WRITE_GENESISCODE_SKILL_PACK_v0.1.md`
13. `docs/spec/WRITE_GENESISCODE_SKILL_DISTRIBUTION_v1.md`
14. `ROADMAP.md` for strategic work or `upgrade_plan.md` for active P0/P1 defects

## Agent-First Onboarding Spine (Required)

1. Language semantics:
`docs/spec/GC_AGENT_CORE_CARD_v0.3.md`, `docs/spec/GC_AGENT_PROFILE_v0.3.json`, intent-selected `docs/spec/GC_AGENT_TASK_CARDS_v0.3.md`, exact symbol and `docs/spec/GC_DIAGNOSTIC_CATALOG_v0.1.json` records, `docs/spec/CLI_TOOLING_BUNDLE_v0.1.md`, `docs/spec/HOST_RUNTIME_BUNDLE_v0.1.md`
2. Runtime profiles:
`docs/spec/RUNTIME_BACKEND_PROFILES_v0.1.md`, `docs/spec/TESTING_BUNDLE_v0.1.md`
3. Packaging and deployment:
`docs/spec/GCPM_BUNDLE_v0.1.md`, `docs/spec/GCPM_WORKFLOW_REPORTS_v0.1.md`
4. Assurance and traceability:
`docs/spec/ASSURANCE_ARTIFACTS_v0.1.md`, `docs/spec/ASSURANCE_PROFILE_PACKS_v0.1.md`, `docs/spec/ASSURANCE_STANDARDS_CROSSWALK_v0.1.md`
5. Active execution risk:
`ROADMAP.md`, `upgrade_plan.md`, `docs/status/REDTEAM_REPORT.md`, `.genesis/perf/selfhost_readiness_report.json`

## Canonical Domain Sources

- CLI/tooling contracts: `docs/spec/CLI_TOOLING_BUNDLE_v0.1.md`
- Project/package manager contracts: `docs/spec/GCPM_BUNDLE_v0.1.md`
- Runtime/capability/host contracts: `docs/spec/HOST_RUNTIME_BUNDLE_v0.1.md`
- GPU/gfx contracts: `docs/spec/GPU_GFX_BUNDLE_v0.1.md`
- GPU compute contracts: `docs/spec/GPU_COMPUTE_BUNDLE_v0.1.md`
- GPU/XR productization templates + lanes: `docs/spec/GPU_COMPUTE_BUNDLE_v0.1.md` (`Productization Kits (Non-Gfx + XR)` section)
- GFX runtime contracts: `docs/spec/GFX_RUNTIME_BUNDLE_v0.1.md`
- Test/perf/release lanes: `docs/spec/TESTING_BUNDLE_v0.1.md`
- Agent authoring entrypoint: `docs/spec/AGENT_AUTHORING_BUNDLE_v0.1.md`
- Versioned skill pack: `docs/spec/WRITE_GENESISCODE_SKILL_PACK_v0.1.md`
- Executable skill distribution kit: `docs/spec/WRITE_GENESISCODE_SKILL_DISTRIBUTION_v1.md`
- Active risk backlog: `upgrade_plan.md`
- Cross-language feature delta: `feature_matrix.md`

## Retrieval Rules (Agent-Facing)

- Prefer bundle docs before split docs.
- Prefer bundle docs before any non-bundle spec page, even if a direct symbol/path match exists.
- Treat split docs with a legacy banner as detail references, not first retrieval targets.
- Do not use superseded top-level docs as normative sources; consult `docs/DEPRECATION_MAP_v0.1.md`.
- Treat `experimental-syntax`, `host-only-operation`, `unavailable-target`, `nondeterministic-facility`, and `out-of-profile-capability` as mandatory fail-closed classes. Use each record's `safeAlternative`; profile/capability negotiation is explicit and cannot be inferred from availability.
- Route failures on exact `genesis/diagnostic/v1/...` IDs or cataloged codes, never prose. Pin `catalogIdentitySha256` for training/evaluation and use `agent-index --diagnostic <exact-code>` rather than loading the full catalog.
