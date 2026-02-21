# GenesisCode Docs Index

Last updated: 2026-02-21

This is the canonical entrypoint for project documentation.

## Start Here

- `docs/AGENT_ONBOARDING_v0.1.md` - minimal stable retrieval path for AI coding agents.
- `docs/GETTING_STARTED.md` - local setup and first workflows.
- `docs/PAPER_v0.2.md` - language thesis and architecture.
- `docs/TECH_HANDOFF.md` - implementation handoff details.
- `README.md` - workspace build/test quickstart.

## Live Status and Planning

- `upgrade_plan.md` - unresolved red-team backlog only.
- `docs/status/REDTEAM_REPORT.md` - active P0/P1 risk summary.
- `docs/status/SELFHOST_CUTOVER.md` - generated selfhost cutover dashboard.
- `feature_matrix.md` - capability comparison vs common languages.

## Core Specs (Normative)

- Canonical bundles (agent-first entrypoints):
  - `docs/spec/CLI_TOOLING_BUNDLE_v0.1.md`
  - `docs/spec/GCPM_BUNDLE_v0.1.md`
  - `docs/spec/HOST_RUNTIME_BUNDLE_v0.1.md`
  - `docs/spec/GPU_GFX_BUNDLE_v0.1.md`
  - `docs/spec/TESTING_BUNDLE_v0.1.md`
- Kernel/evaluator/contracts:
  - `docs/spec/SEALS_DISPATCH_REPLAY.md`
  - `docs/spec/DETERMINISM.md`
  - `docs/spec/TYPES.md`
  - `docs/spec/MODULE_SCOPE.md`
- Tooling/CLI/project manager:
  - `docs/spec/CLI.md`
  - `docs/spec/CLI_JSON_SCHEMAS_v0.1.md`
  - `docs/spec/GCPM_CLI_CONTRACT_v0.1.md`
  - `docs/spec/GCPM_WORKSPACE_v0.1.md`
  - `docs/spec/GCPM_ENV_v0.1.md`
- Effects/policies/runtime:
  - `docs/spec/CAPS_TOML.md`
  - `docs/spec/HOST_ABI.md`
  - `docs/spec/LIMITS.md`
  - `docs/spec/RUNTIME_BACKEND_PROFILES_v0.1.md`
  - `docs/spec/WASI.md`
  - `docs/spec/WASM.md`
- VCS/pkg/registry:
  - `docs/spec/PATCH_SCHEMA.md`
  - `docs/spec/ASSURANCE_ARTIFACTS_v0.1.md`
  - `docs/spec/REGISTRY_POLICY.md`
  - `docs/spec/TRANSPARENCY_LOG.md`

## Graphics, GPU, Concurrency

- `docs/spec/GFX_ARCH.md`
- `docs/spec/GFX_CAPS.md`
- `docs/spec/GPU_COMPUTE_RUNTIME_PROFILE_v0.1.md`
- `docs/spec/GPU_COMPUTE_DEVICE_BRIDGE_v0.1.md`
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
- `docs/spec/PARITY_HARNESS.md` - parity binaries and migration boundaries.
- `old_bootstrap/` - archived bootstrap artifacts and compatibility material.
