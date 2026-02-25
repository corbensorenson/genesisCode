# Feature Matrix Evidence Ledger v0.1

Machine-verifiable capability-to-evidence mapping generated from `feature_matrix.md`.

- Contract kind: `genesis/feature-matrix-evidence-v0.1`
- Capability entries: `30`
- Source matrix: `feature_matrix.md`

| Capability | Evidence Paths | Gate/Test Paths |
| --- | --- | --- |
| Pure deterministic kernel separated from effects | `docs/spec/CLI.md`<br>`docs/spec/SEALS_DISPATCH_REPLAY.md`<br>`docs/spec/DETERMINISM.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_no_user_panics.sh` |
| Canonical CoreForm IR + stable content hash identity | `docs/spec/CLI.md`<br>`docs/spec/COREFORM_CANON_HASH.md` | `scripts/check_upgrade_plan_health.sh` |
| Sealed unforgeable `UNHANDLED`/`EFFECT`/`ERROR` protocol | `docs/spec/CLI.md`<br>`docs/spec/SEALS_DISPATCH_REPLAY.md`<br>`docs/spec/DETERMINISM.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_no_user_panics.sh` |
| Deny-by-default capability policy runtime | `docs/spec/CLI.md` | `scripts/check_upgrade_plan_health.sh` |
| Deterministic effect logs + replay checker | `docs/spec/CLI.md`<br>`docs/spec/SEALS_DISPATCH_REPLAY.md`<br>`docs/spec/DETERMINISM.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_no_user_panics.sh` |
| Built-in semantic VCS (`commit`/`patch`/`refs`/`merge3`) | `docs/spec/CLI.md` | `scripts/check_upgrade_plan_health.sh` |
| Built-in package/project manager (`pkg`/`gcpm`) | `docs/spec/CLI.md`<br>`docs/spec/GCPM_BUNDLE_v0.1.md`<br>`docs/spec/GCPM_JSON_SCHEMAS_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_foundation_stdlib_conformance.sh` |
| Reachability-based artifact GC (`refs` + locks + pins) | `docs/spec/CLI.md`<br>`docs/spec/GCPM_BUNDLE_v0.1.md`<br>`docs/spec/GCPM_JSON_SCHEMAS_v0.1.md`<br>`docs/spec/SELF_HOST_BOUNDARY.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_foundation_stdlib_conformance.sh`<br>`scripts/check_selfhost_boundary.sh`<br>`scripts/check_selfhost_artifact_fresh.sh` |
| Obligation/evidence/attestation-gated publish + ref updates | `docs/spec/CLI.md` | `scripts/check_upgrade_plan_health.sh` |
| Native + WASI + wasm-host runtime surfaces | `docs/spec/CLI.md` | `scripts/check_upgrade_plan_health.sh` |
| Selfhost frontend default in production CLIs | `docs/spec/CLI.md`<br>`docs/spec/SELF_HOST_BOUNDARY.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_selfhost_boundary.sh`<br>`scripts/check_selfhost_artifact_fresh.sh` |
| Full selfhost cutover profile + readiness scorecard | `docs/spec/CLI.md`<br>`docs/spec/SELF_HOST_BOUNDARY.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_selfhost_boundary.sh`<br>`scripts/check_selfhost_artifact_fresh.sh` |
| Strict no-production Rust semantic fallback guard | `docs/spec/CLI.md` | `scripts/check_upgrade_plan_health.sh` |
| CLI + GCPM JSON schema contracts for agent automation | `docs/spec/CLI.md`<br>`docs/spec/GCPM_BUNDLE_v0.1.md`<br>`docs/spec/GCPM_JSON_SCHEMAS_v0.1.md`<br>`docs/spec/CLI_JSON_SCHEMAS_v0.1.md`<br>`docs/spec/AGENT_AUTHORING_BUNDLE_v0.1.md`<br>`docs/spec/AGENT_CAPABILITY_GAUNTLET_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_foundation_stdlib_conformance.sh`<br>`scripts/check_cli_diagnostics_contract.sh`<br>`scripts/check_agent_reference_workflows.sh` |
| Agent index + skill-pack conformance contracts | `docs/spec/CLI.md`<br>`docs/spec/AGENT_AUTHORING_BUNDLE_v0.1.md`<br>`docs/spec/AGENT_CAPABILITY_GAUNTLET_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_agent_reference_workflows.sh` |
| Domain starter registry for agent workflows (27 domains) | `docs/spec/CLI.md`<br>`docs/spec/AGENT_AUTHORING_BUNDLE_v0.1.md`<br>`docs/spec/AGENT_CAPABILITY_GAUNTLET_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_agent_reference_workflows.sh` |
| Agent generative workload parity gates (native vs WASI) | `docs/spec/CLI.md`<br>`docs/spec/AGENT_AUTHORING_BUNDLE_v0.1.md`<br>`docs/spec/AGENT_CAPABILITY_GAUNTLET_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_agent_reference_workflows.sh` |
| Large-workspace agent iteration SLO lane (>=10k modules) | `docs/spec/CLI.md`<br>`docs/spec/GCPM_BUNDLE_v0.1.md`<br>`docs/spec/GCPM_JSON_SCHEMAS_v0.1.md`<br>`docs/spec/AGENT_AUTHORING_BUNDLE_v0.1.md`<br>`docs/spec/AGENT_CAPABILITY_GAUNTLET_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_foundation_stdlib_conformance.sh`<br>`scripts/check_agent_reference_workflows.sh` |
| Concurrency/task replay stress lane | `docs/spec/CLI.md`<br>`docs/spec/CONCURRENCY_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_task_concurrency_stress.sh` |
| GPU compute capability independent of graphics surface | `docs/spec/CLI.md`<br>`docs/spec/GPU_GFX_BUNDLE_v0.1.md`<br>`docs/spec/GPU_COMPUTE_RUNTIME_PROFILE_v0.1.md`<br>`docs/spec/GPU_COMPUTE_BUNDLE_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_gpu_compute_runtime_profile.sh`<br>`scripts/check_gpu_stack_decoupling.sh`<br>`scripts/check_gfx_runtime_profile.sh` |
| Graphics/window/input/audio capability families | `docs/spec/CLI.md`<br>`docs/spec/GPU_GFX_BUNDLE_v0.1.md`<br>`docs/spec/GPU_COMPUTE_RUNTIME_PROFILE_v0.1.md`<br>`docs/spec/GFX_RUNTIME_BUNDLE_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_gpu_compute_runtime_profile.sh`<br>`scripts/check_gpu_stack_decoupling.sh`<br>`scripts/check_gfx_runtime_profile.sh` |
| XR and browser runtime capability families | `docs/spec/CLI.md`<br>`docs/spec/XR_HOST_RUNTIME_v0.1.md`<br>`docs/spec/BROWSER_HOST_RUNTIME_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_webxr_browser_conformance_lane.sh` |
| GPU/XR productization conformance lane | `docs/spec/CLI.md`<br>`docs/spec/GPU_GFX_BUNDLE_v0.1.md`<br>`docs/spec/GPU_COMPUTE_RUNTIME_PROFILE_v0.1.md`<br>`docs/spec/XR_HOST_RUNTIME_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_gpu_compute_runtime_profile.sh`<br>`scripts/check_gpu_stack_decoupling.sh`<br>`scripts/check_gfx_runtime_profile.sh`<br>`scripts/check_webxr_browser_conformance_lane.sh` |
| Host plugin + FFI capability schemas | `docs/spec/CLI.md`<br>`docs/spec/HOST_ABI.md`<br>`docs/spec/PLUGIN_ABI_SCHEMAS_v0.1.md`<br>`docs/spec/CLI_JSON_SCHEMAS_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_host_abi_conformance.sh`<br>`scripts/check_cli_diagnostics_contract.sh` |
| First-party backend bridge for network/process/db/crypto/plugin/ffi | `docs/spec/CLI.md`<br>`docs/spec/HOST_ABI.md`<br>`docs/spec/PLUGIN_ABI_SCHEMAS_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_host_abi_conformance.sh` |
| Stage2 CoreForm->WASM translation-validation path | `docs/spec/CLI.md`<br>`docs/spec/COREFORM_CANON_HASH.md` | `scripts/check_upgrade_plan_health.sh` |
| Deployment target pipeline in core toolchain | `docs/spec/CLI.md`<br>`docs/spec/GCPM_JSON_SCHEMAS_v0.1.md`<br>`docs/spec/GCPM_WORKFLOW_REPORTS_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`crates/gc_cli/tests/cli_pkg_workspace.rs`<br>`examples/agent_deploy_bundle_workflow/workflow.sh` |
| Assurance profile packs + standards crosswalk (DO-178C/NASA/IEC) | `docs/spec/CLI.md`<br>`docs/spec/ASSURANCE_ARTIFACTS_v0.1.md`<br>`docs/spec/ASSURANCE_PROFILE_PACKS_v0.1.md`<br>`docs/spec/ASSURANCE_STANDARDS_CROSSWALK_v0.1.md`<br>`docs/spec/ASSURANCE_STANDARDS_CROSSWALK_v0.1.json` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_assurance_profile_packs.sh`<br>`scripts/check_assurance_standards_crosswalk.sh` |
| Tool qualification lineage + evidence closures | `docs/spec/CLI.md` | `scripts/check_upgrade_plan_health.sh` |
| AI-first modular decomposition + boundary guards | `docs/spec/CLI.md` | `scripts/check_upgrade_plan_health.sh` |
