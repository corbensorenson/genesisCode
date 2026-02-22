# Feature Matrix Evidence Ledger v0.1

Machine-verifiable capability-to-evidence mapping generated from `feature_matrix.md`.

- Contract kind: `genesis/feature-matrix-evidence-v0.1`
- Capability entries: `48`
- Source matrix: `feature_matrix.md`

| Capability | Evidence Paths | Gate/Test Paths |
| --- | --- | --- |
| Pure deterministic kernel separated from effects | `docs/spec/CLI.md`<br>`docs/spec/SEALS_DISPATCH_REPLAY.md`<br>`docs/spec/DETERMINISM.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_no_user_panics.sh` |
| Canonical CoreForm normalization + stable content hashing contract | `docs/spec/CLI.md`<br>`docs/spec/COREFORM_CANON_HASH.md` | `scripts/check_upgrade_plan_health.sh` |
| Unforgeable protocol values (sealed UNHANDLED/EFFECT/ERROR) | `docs/spec/CLI.md`<br>`docs/spec/SEALS_DISPATCH_REPLAY.md`<br>`docs/spec/DETERMINISM.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_no_user_panics.sh` |
| Deny-by-default capability policy runtime | `docs/spec/CLI.md` | `scripts/check_upgrade_plan_health.sh` |
| Deterministic effect logs + replay checker | `docs/spec/CLI.md`<br>`docs/spec/SEALS_DISPATCH_REPLAY.md`<br>`docs/spec/DETERMINISM.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_no_user_panics.sh` |
| Obligations + evidence artifacts in core workflow | `docs/spec/CLI.md`<br>`docs/spec/SELF_HOST_BOUNDARY.md`<br>`docs/spec/AGENT_AUTHORING_BUNDLE_v0.1.md`<br>`docs/spec/AGENT_CAPABILITY_GAUNTLET_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_selfhost_boundary.sh`<br>`scripts/check_selfhost_artifact_fresh.sh`<br>`scripts/check_agent_reference_workflows.sh` |
| Language-native semantic VCS DAG + refs + bundles | `docs/spec/CLI.md` | `scripts/check_upgrade_plan_health.sh` |
| Built-in package/project manager | `docs/spec/CLI.md`<br>`docs/spec/GCPM_BUNDLE_v0.1.md`<br>`docs/spec/GCPM_JSON_SCHEMAS_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_foundation_stdlib_conformance.sh` |
| Strict selfhost frontend default in production CLI | `docs/spec/CLI.md`<br>`docs/spec/SELF_HOST_BOUNDARY.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_selfhost_boundary.sh`<br>`scripts/check_selfhost_artifact_fresh.sh` |
| Explicit selfhost-only execution mode | `docs/spec/CLI.md`<br>`docs/spec/SELF_HOST_BOUNDARY.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_selfhost_boundary.sh`<br>`scripts/check_selfhost_artifact_fresh.sh` |
| Fully self-hosted toolchain with zero bootstrap-language dependency | `docs/spec/CLI.md`<br>`docs/spec/SELF_HOST_BOUNDARY.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_selfhost_boundary.sh`<br>`scripts/check_selfhost_artifact_fresh.sh` |
| Artifact-only bootstrap default across WASM host APIs | `docs/spec/CLI.md`<br>`docs/spec/SELF_HOST_BOUNDARY.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_selfhost_boundary.sh`<br>`scripts/check_selfhost_artifact_fresh.sh` |
| Deterministic concurrency/task API with replay semantics | `docs/spec/CLI.md`<br>`docs/spec/SEALS_DISPATCH_REPLAY.md`<br>`docs/spec/DETERMINISM.md`<br>`docs/spec/CONCURRENCY_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_no_user_panics.sh`<br>`scripts/check_task_concurrency_stress.sh` |
| Multithreaded runtime task execution | `docs/spec/CLI.md`<br>`docs/spec/CONCURRENCY_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_task_concurrency_stress.sh` |
| GPU compute + graphics capability surfaces | `docs/spec/CLI.md`<br>`docs/spec/GPU_GFX_BUNDLE_v0.1.md`<br>`docs/spec/GPU_COMPUTE_RUNTIME_PROFILE_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_gpu_compute_runtime_profile.sh` |
| Media/asset pipeline contracts for AI-generated build lanes | `docs/spec/CLI.md` | `scripts/check_upgrade_plan_health.sh` |
| Device-backed GPU compute required in release profile | `docs/spec/CLI.md`<br>`docs/spec/GPU_GFX_BUNDLE_v0.1.md`<br>`docs/spec/GPU_COMPUTE_RUNTIME_PROFILE_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_gpu_compute_runtime_profile.sh` |
| Network + process execution as policy-gated capabilities | `docs/spec/CLI.md` | `scripts/check_upgrade_plan_health.sh` |
| Filesystem management capability surface (`stat/list/mkdir/rename/remove`) | `docs/spec/CLI.md` | `scripts/check_upgrade_plan_health.sh` |
| Process lifecycle + stdio streaming primitives | `docs/spec/CLI.md` | `scripts/check_upgrade_plan_health.sh` |
| Raw socket/stream networking primitives | `docs/spec/CLI.md` | `scripts/check_upgrade_plan_health.sh` |
| Inbound server networking primitives (listen/accept/http-serve/ws-accept) | `docs/spec/CLI.md` | `scripts/check_upgrade_plan_health.sh` |
| Generic host extension/FFI capability ABI | `docs/spec/CLI.md`<br>`docs/spec/HOST_ABI.md`<br>`docs/spec/PLUGIN_ABI_SCHEMAS_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_host_abi_conformance.sh` |
| Plugin command surface hardening (command allowlists + bridge digest pinning) | `docs/spec/CLI.md`<br>`docs/spec/HOST_ABI.md`<br>`docs/spec/PLUGIN_ABI_SCHEMAS_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_host_abi_conformance.sh` |
| Browser runtime host profile for wasm-hosted apps | `docs/spec/CLI.md`<br>`docs/spec/BROWSER_HOST_RUNTIME_v0.1.md` | `scripts/check_upgrade_plan_health.sh` |
| WebXR runtime primitives (session/frame/input/haptics) | `docs/spec/CLI.md`<br>`docs/spec/XR_HOST_RUNTIME_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_webxr_browser_conformance_lane.sh` |
| Advanced XR spatial primitives (anchors/hands/mesh/layers) | `docs/spec/CLI.md`<br>`docs/spec/XR_HOST_RUNTIME_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_webxr_browser_conformance_lane.sh` |
| Durable data capability family (`io/db::*`) | `docs/spec/CLI.md` | `scripts/check_upgrade_plan_health.sh` |
| First-class cryptography capability family | `docs/spec/CLI.md` | `scripts/check_upgrade_plan_health.sh` |
| WASM runtime APIs | `docs/spec/CLI.md` | `scripts/check_upgrade_plan_health.sh` |
| WASI CLI support | `docs/spec/CLI.md` | `scripts/check_upgrade_plan_health.sh` |
| Schema-stable JSON CLI contracts for agents | `docs/spec/CLI.md`<br>`docs/spec/CLI_JSON_SCHEMAS_v0.1.md`<br>`docs/spec/AGENT_AUTHORING_BUNDLE_v0.1.md`<br>`docs/spec/AGENT_CAPABILITY_GAUNTLET_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_cli_diagnostics_contract.sh`<br>`scripts/check_agent_reference_workflows.sh` |
| Deployment/bundle target pipeline in core toolchain | `docs/spec/CLI.md` | `scripts/check_upgrade_plan_health.sh` |
| Workspace semantic graph/refactor API for automation | `docs/spec/CLI.md`<br>`docs/spec/GCPM_BUNDLE_v0.1.md`<br>`docs/spec/GCPM_JSON_SCHEMAS_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_foundation_stdlib_conformance.sh` |
| Machine-consumable agent authoring contract | `docs/spec/CLI.md`<br>`docs/spec/CLI_JSON_SCHEMAS_v0.1.md`<br>`docs/spec/WRITE_GENESISCODE_SKILL_PACK_v0.1.md`<br>`docs/spec/WRITE_GENESISCODE_SKILL_PACK_v0.1.json`<br>`docs/spec/AGENT_AUTHORING_BUNDLE_v0.1.md`<br>`docs/spec/AGENT_CAPABILITY_GAUNTLET_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_cli_diagnostics_contract.sh`<br>`scripts/check_write_genesiscode_skill_pack.sh`<br>`scripts/check_agent_reference_workflows.sh` |
| Supply-chain signing + transparency in primary CLI | `docs/spec/CLI.md` | `scripts/check_upgrade_plan_health.sh` |
| Local artifact GC by refs/locks/pins reachability | `docs/spec/CLI.md`<br>`docs/spec/GCPM_BUNDLE_v0.1.md`<br>`docs/spec/GCPM_JSON_SCHEMAS_v0.1.md`<br>`docs/spec/SELF_HOST_BOUNDARY.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_foundation_stdlib_conformance.sh`<br>`scripts/check_selfhost_boundary.sh`<br>`scripts/check_selfhost_artifact_fresh.sh` |
| Runtime backend profile selection through project manager workflows | `docs/spec/CLI.md`<br>`docs/spec/AGENT_AUTHORING_BUNDLE_v0.1.md`<br>`docs/spec/AGENT_CAPABILITY_GAUNTLET_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_agent_reference_workflows.sh` |
| Deterministic non-gfx runtime profiling in core workflow | `docs/spec/CLI.md`<br>`docs/spec/SEALS_DISPATCH_REPLAY.md`<br>`docs/spec/DETERMINISM.md`<br>`docs/spec/GPU_GFX_BUNDLE_v0.1.md`<br>`docs/spec/GPU_COMPUTE_RUNTIME_PROFILE_v0.1.md`<br>`docs/spec/AGENT_AUTHORING_BUNDLE_v0.1.md`<br>`docs/spec/AGENT_CAPABILITY_GAUNTLET_v0.1.md`<br>`docs/spec/TEST_EXECUTION_PROFILES_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_no_user_panics.sh`<br>`scripts/check_gpu_compute_runtime_profile.sh`<br>`scripts/check_agent_reference_workflows.sh`<br>`scripts/check_perf_budgets.sh` |
| Generative workload regression gates with enforced historical baselines | `docs/spec/CLI.md` | `scripts/check_upgrade_plan_health.sh` |
| Enforced runtime wall-time budgets for strict/full profile lanes | `docs/spec/CLI.md`<br>`docs/spec/TEST_EXECUTION_PROFILES_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_perf_budgets.sh` |
| Perf/hot-path gate operability under constrained local disk headroom | `docs/spec/CLI.md`<br>`docs/spec/TEST_EXECUTION_PROFILES_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_perf_budgets.sh` |
| Health gate lock-aware cargo scheduling + shared build cache target | `docs/spec/CLI.md`<br>`docs/spec/GCPM_BUNDLE_v0.1.md`<br>`docs/spec/GCPM_JSON_SCHEMAS_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_foundation_stdlib_conformance.sh` |
| Bidirectional requirements traceability (system/HLR/LLR -> code -> tests -> artifact) | `docs/spec/CLI.md`<br>`docs/spec/SELF_HOST_BOUNDARY.md`<br>`docs/spec/ASSURANCE_ARTIFACTS_v0.1.md`<br>`docs/spec/ASSURANCE_PROFILE_PACKS_v0.1.md`<br>`docs/spec/ASSURANCE_STANDARDS_CROSSWALK_v0.1.md`<br>`docs/spec/ASSURANCE_STANDARDS_CROSSWALK_v0.1.json` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_selfhost_boundary.sh`<br>`scripts/check_selfhost_artifact_fresh.sh`<br>`scripts/check_assurance_profile_packs.sh`<br>`scripts/check_assurance_standards_crosswalk.sh` |
| Structural coverage profiles (decision/MC/DC) | `docs/spec/CLI.md`<br>`docs/spec/ASSURANCE_ARTIFACTS_v0.1.md`<br>`docs/spec/ASSURANCE_PROFILE_PACKS_v0.1.md`<br>`docs/spec/ASSURANCE_STANDARDS_CROSSWALK_v0.1.md`<br>`docs/spec/ASSURANCE_STANDARDS_CROSSWALK_v0.1.json` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_assurance_profile_packs.sh`<br>`scripts/check_assurance_standards_crosswalk.sh` |
| Qualified-tool evidence bundles for regulated release | `docs/spec/CLI.md`<br>`docs/spec/ASSURANCE_ARTIFACTS_v0.1.md`<br>`docs/spec/ASSURANCE_PROFILE_PACKS_v0.1.md`<br>`docs/spec/ASSURANCE_STANDARDS_CROSSWALK_v0.1.md`<br>`docs/spec/ASSURANCE_STANDARDS_CROSSWALK_v0.1.json` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_assurance_profile_packs.sh`<br>`scripts/check_assurance_standards_crosswalk.sh` |
| Independent verifier role-separation policy enforcement | `docs/spec/CLI.md` | `scripts/check_upgrade_plan_health.sh` |
| Standards-oriented assurance profile packs (DO-178C/NASA/IEC) | `docs/spec/CLI.md`<br>`docs/spec/ASSURANCE_ARTIFACTS_v0.1.md`<br>`docs/spec/ASSURANCE_PROFILE_PACKS_v0.1.md`<br>`docs/spec/ASSURANCE_STANDARDS_CROSSWALK_v0.1.md`<br>`docs/spec/ASSURANCE_STANDARDS_CROSSWALK_v0.1.json` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_assurance_profile_packs.sh`<br>`scripts/check_assurance_standards_crosswalk.sh` |
