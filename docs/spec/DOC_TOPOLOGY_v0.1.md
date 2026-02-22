# Documentation Topology v0.1

Canonical documentation source-of-truth map for AI-first GenesisCode authoring.

## Authoring

- Canonical root: `docs/spec/AGENT_AUTHORING_BUNDLE_v0.1.md`
- Skill contract: `docs/spec/WRITE_GENESISCODE_SKILL_v0.1.md`
- Skill pack: `docs/spec/WRITE_GENESISCODE_SKILL_PACK_v0.1.md`
- Executable skill conformance: `docs/spec/WRITE_GENESISCODE_SKILL_CONFORMANCE_v0.1.md`

Owner: Language + agent authoring maintainers.

## Runtime

- Canonical root: `docs/spec/HOST_RUNTIME_BUNDLE_v0.1.md`
- GPU/graphics bundle: `docs/spec/GPU_GFX_BUNDLE_v0.1.md`
- GPU compute bundle: `docs/spec/GPU_COMPUTE_BUNDLE_v0.1.md`
- GFX runtime bundle: `docs/spec/GFX_RUNTIME_BUNDLE_v0.1.md`
- Runtime backend policy: `docs/spec/RUNTIME_BACKEND_PROFILES_v0.1.md`

Owner: Runtime/effects maintainers.

## Assurance

- Canonical root: `docs/spec/ASSURANCE_ARTIFACTS_v0.1.md`
- Profile packs: `docs/spec/ASSURANCE_PROFILE_PACKS_v0.1.md`
- Standards crosswalk: `docs/spec/ASSURANCE_STANDARDS_CROSSWALK_v0.1.md`

Owner: Assurance + release maintainers.

## Operations

- Live backlog: `upgrade_plan.md`
- Active risk rollup: `docs/status/REDTEAM_REPORT.md`
- Capability comparison ledger: `feature_matrix.md`
- Machine-readable selfhost readiness scorecard: `.genesis/perf/selfhost_readiness_report.json`
- Canonical docs index: `docs/INDEX.md`

Owner: Release ops maintainers.

## Update Workflow

1. Edit canonical topology-owned docs first.
2. Update derived docs (`feature_matrix.md`, status docs, skill pointers) in the same change.
3. Run drift checks:
   - `bash scripts/check_doc_topology_drift.sh`
   - `bash scripts/check_feature_matrix_gap_hygiene.sh`
   - `bash scripts/check_redteam_report.sh`
4. Only then mark backlog items complete in `upgrade_plan.md`.
