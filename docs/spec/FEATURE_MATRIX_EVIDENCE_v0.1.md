# Feature Matrix Evidence Ledger v0.1

Machine-verifiable capability-to-evidence mapping generated from `feature_matrix.md`.

- Contract kind: `genesis/feature-matrix-evidence-v0.1`
- Capability entries: `22`
- Source matrix: `feature_matrix.md`

| Capability | Evidence Paths | Gate/Test Paths |
| --- | --- | --- |
| Pure deterministic kernel separated from effects | `docs/spec/CLI.md`<br>`docs/spec/SEALS_DISPATCH_REPLAY.md`<br>`docs/spec/DETERMINISM.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_no_user_panics.sh` |
| Canonical language form + stable semantic hashing | `docs/spec/CLI.md`<br>`docs/spec/COREFORM_CANON_HASH.md` | `scripts/check_upgrade_plan_health.sh` |
| Unforgeable protocol values (sealed error/effect channels) | `docs/spec/CLI.md`<br>`docs/spec/SEALS_DISPATCH_REPLAY.md`<br>`docs/spec/DETERMINISM.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_no_user_panics.sh` |
| Deny-by-default capability runtime | `docs/spec/CLI.md` | `scripts/check_upgrade_plan_health.sh` |
| Deterministic effect logs + replay checker | `docs/spec/CLI.md`<br>`docs/spec/SEALS_DISPATCH_REPLAY.md`<br>`docs/spec/DETERMINISM.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_no_user_panics.sh` |
| Obligations + evidence artifacts in core workflow | `docs/spec/CLI.md`<br>`docs/spec/SELF_HOST_BOUNDARY.md`<br>`docs/spec/AGENT_AUTHORING_BUNDLE_v0.1.md`<br>`docs/spec/AGENT_CAPABILITY_GAUNTLET_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_selfhost_boundary.sh`<br>`scripts/check_selfhost_artifact_fresh.sh`<br>`scripts/check_agent_reference_workflows.sh` |
| Language-native semantic VCS graph + refs + bundles | `docs/spec/CLI.md` | `scripts/check_upgrade_plan_health.sh` |
| Built-in package/project manager | `docs/spec/CLI.md`<br>`docs/spec/GCPM_BUNDLE_v0.1.md`<br>`docs/spec/GCPM_JSON_SCHEMAS_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_foundation_stdlib_conformance.sh` |
| Deployment/bundle target pipeline in core toolchain | `docs/spec/CLI.md`<br>`docs/spec/GCPM_JSON_SCHEMAS_v0.1.md`<br>`docs/spec/GCPM_WORKFLOW_REPORTS_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`crates/gc_cli/tests/cli_pkg_workspace.rs`<br>`examples/agent_deploy_bundle_workflow/workflow.sh` |
| Real native deploy packaging/execution artifacts | `docs/spec/CLI.md`<br>`docs/spec/SELF_HOST_BOUNDARY.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_selfhost_boundary.sh`<br>`scripts/check_selfhost_artifact_fresh.sh` |
| Strict selfhost frontend default in production binaries | `docs/spec/CLI.md`<br>`docs/spec/SELF_HOST_BOUNDARY.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_selfhost_boundary.sh`<br>`scripts/check_selfhost_artifact_fresh.sh` |
| Full no-bootstrap-language self-host closure | `docs/spec/CLI.md`<br>`docs/spec/SELF_HOST_BOUNDARY.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_selfhost_boundary.sh`<br>`scripts/check_selfhost_artifact_fresh.sh` |
| Machine-readable agent planning index + schema contracts | `docs/spec/CLI.md`<br>`docs/spec/CLI_JSON_SCHEMAS_v0.1.md`<br>`docs/spec/AGENT_AUTHORING_BUNDLE_v0.1.md`<br>`docs/spec/AGENT_CAPABILITY_GAUNTLET_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_cli_diagnostics_contract.sh`<br>`scripts/check_agent_reference_workflows.sh` |
| Semantic edit/refactor primitives as first-class CLI surface | `docs/spec/CLI.md` | `scripts/check_upgrade_plan_health.sh` |
| LSP/editor server surface | `docs/spec/CLI.md` | `scripts/check_upgrade_plan_health.sh` |
| Interactive debugger/breakpoint surface | `docs/spec/CLI.md` | `scripts/check_upgrade_plan_health.sh` |
| GPU compute + graphics capability surfaces | `docs/spec/CLI.md`<br>`docs/spec/GPU_GFX_BUNDLE_v0.1.md`<br>`docs/spec/GPU_COMPUTE_RUNTIME_PROFILE_v0.1.md`<br>`docs/spec/GPU_COMPUTE_BUNDLE_v0.1.md`<br>`docs/spec/GFX_RUNTIME_BUNDLE_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_gpu_compute_runtime_profile.sh`<br>`scripts/check_gpu_stack_decoupling.sh`<br>`scripts/check_gfx_runtime_profile.sh` |
| Deterministic task concurrency runtime with replay semantics | `docs/spec/CLI.md`<br>`docs/spec/SEALS_DISPATCH_REPLAY.md`<br>`docs/spec/DETERMINISM.md`<br>`docs/spec/CONCURRENCY_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_no_user_panics.sh`<br>`scripts/check_task_concurrency_stress.sh` |
| WASM runtime + WASI CLI surfaces | `docs/spec/CLI.md` | `scripts/check_upgrade_plan_health.sh` |
| Supply-chain policy + provenance gating in primary CLI | `docs/spec/CLI.md` | `scripts/check_upgrade_plan_health.sh` |
| Local artifact GC by semantic reachability (refs/locks/pins) | `docs/spec/CLI.md`<br>`docs/spec/GCPM_BUNDLE_v0.1.md`<br>`docs/spec/GCPM_JSON_SCHEMAS_v0.1.md`<br>`docs/spec/SELF_HOST_BOUNDARY.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_foundation_stdlib_conformance.sh`<br>`scripts/check_selfhost_boundary.sh`<br>`scripts/check_selfhost_artifact_fresh.sh` |
| Regulated assurance profile packs in core workflow | `docs/spec/CLI.md`<br>`docs/spec/AGENT_AUTHORING_BUNDLE_v0.1.md`<br>`docs/spec/AGENT_CAPABILITY_GAUNTLET_v0.1.md`<br>`docs/spec/ASSURANCE_ARTIFACTS_v0.1.md`<br>`docs/spec/ASSURANCE_PROFILE_PACKS_v0.1.md`<br>`docs/spec/ASSURANCE_STANDARDS_CROSSWALK_v0.1.md`<br>`docs/spec/ASSURANCE_STANDARDS_CROSSWALK_v0.1.json` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_agent_reference_workflows.sh`<br>`scripts/check_assurance_profile_packs.sh`<br>`scripts/check_assurance_standards_crosswalk.sh` |
