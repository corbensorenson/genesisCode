# Feature Matrix Evidence Ledger v0.1

Machine-verifiable capability-to-evidence mapping generated from `feature_matrix.md`.

- Contract kind: `genesis/feature-matrix-evidence-v0.1`
- Capability entries: `23`
- Source matrix: `feature_matrix.md`

| Capability | Evidence Paths | Gate/Test Paths |
| --- | --- | --- |
| Pure deterministic kernel separated from effects | `docs/spec/CLI.md`<br>`docs/spec/SEALS_DISPATCH_REPLAY.md`<br>`docs/spec/DETERMINISM.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_no_user_panics.sh` |
| Canonical CoreForm + stable semantic hash identity | `docs/spec/CLI.md`<br>`docs/spec/COREFORM_CANON_HASH.md` | `scripts/check_upgrade_plan_health.sh` |
| Unforgeable sealed effect/error protocol | `docs/spec/CLI.md`<br>`docs/spec/SEALS_DISPATCH_REPLAY.md`<br>`docs/spec/DETERMINISM.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_no_user_panics.sh` |
| Deny-by-default capability policy runtime | `docs/spec/CLI.md` | `scripts/check_upgrade_plan_health.sh` |
| Deterministic effect logs + replay mismatch fail-fast | `docs/spec/CLI.md`<br>`docs/spec/SEALS_DISPATCH_REPLAY.md`<br>`docs/spec/DETERMINISM.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_no_user_panics.sh` |
| Language-native semantic VCS graph (`commit/refs/patch`) | `docs/spec/CLI.md` | `scripts/check_upgrade_plan_health.sh` |
| Built-in package/project manager (`pkg` / `gcpm`) | `docs/spec/CLI.md`<br>`docs/spec/GCPM_BUNDLE_v0.1.md`<br>`docs/spec/GCPM_JSON_SCHEMAS_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_foundation_stdlib_conformance.sh` |
| Agent planning schema (`cli-schema`, `agent-index`) | `docs/spec/CLI.md`<br>`docs/spec/CLI_JSON_SCHEMAS_v0.1.md`<br>`docs/spec/AGENT_AUTHORING_BUNDLE_v0.1.md`<br>`docs/spec/AGENT_CAPABILITY_GAUNTLET_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_cli_diagnostics_contract.sh`<br>`scripts/check_agent_reference_workflows.sh` |
| Semantic edit/refactor CLI (`semantic-edit`) | `docs/spec/CLI.md` | `scripts/check_upgrade_plan_health.sh` |
| Deterministic task concurrency primitives | `docs/spec/CLI.md`<br>`docs/spec/SEALS_DISPATCH_REPLAY.md`<br>`docs/spec/DETERMINISM.md`<br>`docs/spec/CONCURRENCY_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_no_user_panics.sh`<br>`scripts/check_task_concurrency_stress.sh` |
| GPU compute capability independent of graphics surface | `docs/spec/CLI.md`<br>`docs/spec/GPU_GFX_BUNDLE_v0.1.md`<br>`docs/spec/GPU_COMPUTE_RUNTIME_PROFILE_v0.1.md`<br>`docs/spec/GPU_COMPUTE_BUNDLE_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_gpu_compute_runtime_profile.sh`<br>`scripts/check_gpu_stack_decoupling.sh`<br>`scripts/check_gfx_runtime_profile.sh` |
| GPU device-runtime strict lane in default gauntlets | `docs/spec/CLI.md`<br>`docs/spec/GPU_GFX_BUNDLE_v0.1.md`<br>`docs/spec/GPU_COMPUTE_RUNTIME_PROFILE_v0.1.md`<br>`docs/spec/AGENT_AUTHORING_BUNDLE_v0.1.md`<br>`docs/spec/AGENT_CAPABILITY_GAUNTLET_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_gpu_compute_runtime_profile.sh`<br>`scripts/check_gpu_stack_decoupling.sh`<br>`scripts/check_gfx_runtime_profile.sh`<br>`scripts/check_agent_reference_workflows.sh` |
| Graphics/window/input/audio capability families | `docs/spec/CLI.md`<br>`docs/spec/GPU_GFX_BUNDLE_v0.1.md`<br>`docs/spec/GPU_COMPUTE_RUNTIME_PROFILE_v0.1.md`<br>`docs/spec/GFX_RUNTIME_BUNDLE_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_gpu_compute_runtime_profile.sh`<br>`scripts/check_gpu_stack_decoupling.sh`<br>`scripts/check_gfx_runtime_profile.sh` |
| Deployment target pipeline in core toolchain | `docs/spec/CLI.md`<br>`docs/spec/GCPM_JSON_SCHEMAS_v0.1.md`<br>`docs/spec/GCPM_WORKFLOW_REPORTS_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`crates/gc_cli/tests/cli_pkg_workspace.rs`<br>`examples/agent_deploy_bundle_workflow/workflow.sh` |
| Native platform packaging/execution adapters | `docs/spec/CLI.md` | `scripts/check_upgrade_plan_health.sh` |
| Strict selfhost frontend default in production binaries | `docs/spec/CLI.md`<br>`docs/spec/SELF_HOST_BOUNDARY.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_selfhost_boundary.sh`<br>`scripts/check_selfhost_artifact_fresh.sh` |
| Full selfhost closure with minimal bounded Rust TCB | `docs/spec/CLI.md`<br>`docs/spec/SELF_HOST_BOUNDARY.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_selfhost_boundary.sh`<br>`scripts/check_selfhost_artifact_fresh.sh` |
| WASI CLI parity with native CLI for registry hosting | `docs/spec/CLI.md` | `scripts/check_upgrade_plan_health.sh` |
| Type/effect system maturity for large generated codebases | `docs/spec/CLI.md` | `scripts/check_upgrade_plan_health.sh` |
| Interactive deterministic debugger surface | `docs/spec/CLI.md`<br>`docs/spec/SEALS_DISPATCH_REPLAY.md`<br>`docs/spec/DETERMINISM.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_no_user_panics.sh` |
| LSP/editor server for human IDE workflows | `docs/spec/CLI.md`<br>`docs/spec/AGENT_AUTHORING_BUNDLE_v0.1.md`<br>`docs/spec/AGENT_CAPABILITY_GAUNTLET_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_agent_reference_workflows.sh` |
| Policy-gated supply-chain provenance workflow | `docs/spec/CLI.md`<br>`docs/spec/AGENT_AUTHORING_BUNDLE_v0.1.md`<br>`docs/spec/AGENT_CAPABILITY_GAUNTLET_v0.1.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_agent_reference_workflows.sh` |
| Reachability-based artifact GC (refs/locks/pins) | `docs/spec/CLI.md`<br>`docs/spec/GCPM_BUNDLE_v0.1.md`<br>`docs/spec/GCPM_JSON_SCHEMAS_v0.1.md`<br>`docs/spec/SELF_HOST_BOUNDARY.md` | `scripts/check_upgrade_plan_health.sh`<br>`scripts/check_foundation_stdlib_conformance.sh`<br>`scripts/check_selfhost_boundary.sh`<br>`scripts/check_selfhost_artifact_fresh.sh` |
