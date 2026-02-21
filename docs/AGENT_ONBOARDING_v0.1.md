# Agent Onboarding Path v0.1

Last updated: 2026-02-21

Purpose: provide a minimal, stable retrieval path for AI coding agents.

## Canonical Read Order (Required)

1. `docs/INDEX.md`
2. `docs/spec/CLI_TOOLING_BUNDLE_v0.1.md`
3. `docs/spec/GCPM_BUNDLE_v0.1.md`
4. `docs/spec/HOST_RUNTIME_BUNDLE_v0.1.md`
5. `docs/spec/TESTING_BUNDLE_v0.1.md`
6. `upgrade_plan.md`

## Canonical Domain Sources

- CLI/tooling contracts: `docs/spec/CLI_TOOLING_BUNDLE_v0.1.md`
- Project/package manager contracts: `docs/spec/GCPM_BUNDLE_v0.1.md`
- Runtime/capability/host contracts: `docs/spec/HOST_RUNTIME_BUNDLE_v0.1.md`
- GPU/gfx contracts: `docs/spec/GPU_GFX_BUNDLE_v0.1.md`
- Test/perf/release lanes: `docs/spec/TESTING_BUNDLE_v0.1.md`
- Active risk backlog: `upgrade_plan.md`
- Cross-language feature delta: `feature_matrix.md`

## Retrieval Rules (Agent-Facing)

- Prefer bundle docs before split docs.
- Treat split docs with a legacy banner as detail references, not first retrieval targets.
- Do not use superseded top-level docs as normative sources; consult `docs/DEPRECATION_MAP_v0.1.md`.
