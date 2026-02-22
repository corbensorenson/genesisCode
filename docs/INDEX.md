# GenesisCode Docs Index

Last updated: 2026-02-22

This is the canonical entrypoint for project documentation.

## Start Here

- `docs/AGENT_ONBOARDING_v0.1.md` - single agent-first onboarding spine (semantics, runtime, packaging, assurance, deployment).
- `docs/GETTING_STARTED.md` - local setup and first workflows.
- `docs/PAPER_v0.2.md` - language thesis and architecture.
- `docs/TECH_HANDOFF.md` - implementation handoff details.
- `README.md` - workspace build/test quickstart.

## Live Status and Planning

- `upgrade_plan.md` - unresolved red-team backlog only.
- `docs/status/REDTEAM_REPORT.md` - active P0/P1 risk summary.
- `docs/status/SELFHOST_CUTOVER.md` - generated selfhost cutover dashboard.
- `.genesis/perf/selfhost_readiness_report.json` - machine-readable selfhost readiness scorecard.
- `.genesis/perf/doc_complexity_report.json` - machine-readable docs complexity budget report.
- `.genesis/perf/selfhost_gc_migration_plan_report.json` - machine-readable migration-plan drift report for high-churn selfhost surfaces.
- `feature_matrix.md` - capability comparison vs common languages.
- `docs/spec/DOC_TOPOLOGY_v0.1.md` - canonical documentation topology and drift contract.
- `docs/spec/DOC_COMPLEXITY_TARGETS_v0.1.md` - numeric docs complexity targets.
- `docs/spec/DOC_LEAF_OWNERSHIP_v0.1.md` - ownership + canonical source registry for retained top-level leaf docs.

## Core Specs (Normative)

- Canonical bundles (agent-first entrypoints):
  - `docs/spec/CLI_TOOLING_BUNDLE_v0.1.md`
  - `docs/spec/GCPM_BUNDLE_v0.1.md`
  - `docs/spec/HOST_RUNTIME_BUNDLE_v0.1.md`
  - `docs/spec/GPU_GFX_BUNDLE_v0.1.md`
  - `docs/spec/GPU_COMPUTE_BUNDLE_v0.1.md`
  - `docs/spec/GFX_RUNTIME_BUNDLE_v0.1.md`
  - `docs/spec/TESTING_BUNDLE_v0.1.md`
  - `docs/spec/AGENT_AUTHORING_BUNDLE_v0.1.md`
  - `docs/spec/WRITE_GENESISCODE_SKILL_v0.1.md`
  - `docs/spec/WRITE_GENESISCODE_SKILL_PACK_v0.1.md`
  - `docs/spec/WRITE_GENESISCODE_SKILL_PACK_v0.1.json`
  - `docs/spec/WRITE_GENESISCODE_SKILL_DISTRIBUTION_v1.md`
- Consolidation note:
  - low-signal split schema/index docs were merged into canonical specs; see
    `docs/DEPRECATION_MAP_v0.1.md` for the merged mapping.
- Kernel/evaluator/contracts:
  - `docs/spec/SEALS_DISPATCH_REPLAY.md`
  - `docs/spec/DETERMINISM.md`
  - `docs/spec/TYPES.md`
  - `docs/spec/MODULE_SCOPE.md`
- Tooling/CLI/project manager:
  - `docs/spec/CLI.md`
  - `docs/spec/CLI_JSON_SCHEMAS_v0.1.md`
- Effects/policies/runtime:
  - `docs/spec/CAPS_TOML.md`
  - `docs/spec/HOST_ABI.md`
  - `docs/spec/BROWSER_HOST_RUNTIME_v0.1.md`
  - `docs/spec/PLUGIN_ABI_SCHEMAS_v0.1.md`
  - `docs/spec/DOMAIN_KITS_v0.1.md`
  - `docs/spec/RUNTIME_BACKEND_PROFILES_v0.1.md`
  - `docs/spec/WASI.md`
  - `docs/spec/WASM.md`
  - `docs/spec/AGENT_GENERATIVE_WORKLOADS_v0.1.md`
  - `docs/spec/SELFHOST_READINESS_SCORECARD_v0.1.md`
  - `docs/spec/GC_MODULE_BOUNDARIES_v0.1.md`
- VCS/pkg/registry:
  - `docs/spec/PATCH_SCHEMA.md`
  - `docs/spec/ASSURANCE_ARTIFACTS_v0.1.md`
  - `docs/spec/ASSURANCE_PROFILE_PACKS_v0.1.md`
  - `docs/spec/ASSURANCE_STANDARDS_CROSSWALK_v0.1.md`
  - `docs/spec/ASSURANCE_STANDARDS_CROSSWALK_v0.1.json`
  - `docs/spec/REGISTRY_POLICY.md`
  - `docs/spec/TRANSPARENCY_LOG.md`

## Graphics, GPU, Concurrency

- `docs/spec/GFX_CAPS.md`
- `docs/spec/GPU_COMPUTE_BUNDLE_v0.1.md`
- `docs/spec/GFX_RUNTIME_BUNDLE_v0.1.md`
- `docs/spec/GPU_COMPUTE_RUNTIME_PROFILE_v0.1.md`
- `docs/spec/GPU_GFX_BUNDLE_v0.1.md` (`Demo Workloads` section for runnable `.gc` gfx demos)
- `docs/spec/GPU_COMPUTE_BUNDLE_v0.1.md` (`Productization Kits (Non-Gfx + XR)` section)
- `docs/spec/CONCURRENCY_v0.1.md`
- `docs/spec/CONCURRENCY_GPU_SLO_v0.1.md`

## Policy Profiles

- `docs/policies/README.md`
- `docs/policies/gpu_device_runtime_caps_v0.1.toml`
- `docs/policies/gfx_desktop_first_party_caps_v0.1.toml`
- `docs/policies/gpu_compute_bridge_device_caps_v0.1.toml`
- `docs/policies/gpu_compute_bridge_fallback_caps_v0.1.toml`

## Legacy/Bootstrap Reference

- `docs/DEPRECATION_MAP_v0.1.md` - explicit superseded/overlapping doc mapping.
- `docs/spec/BOOTSTRAP_OLD.md` - bootstrap/parity historical reference only.
- `old_bootstrap/` - archived bootstrap artifacts and compatibility material.
