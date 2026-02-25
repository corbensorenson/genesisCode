# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-25

Scope:
- Track only unresolved upgrades required for AI-first authoring reliability, selfhost closure, and productization trust.
- Keep this file machine-syncable with `.genesis/perf/selfhost_readiness_report.json` and `feature_matrix.md`.
- Keep completed work out of this file (git history + perf artifacts are closure evidence).

Open checklist items: 5

## Critical Path

- [ ] P0.1 Expand stage2 translation-validation coverage so selfhost/agent modules stop hitting `Stage2CompileError::Unsupported` for valid CoreForm programs.
- [ ] P0.2 Complete first-party backend bridge semantics for `io/net::*` + `sys/process::*` lifecycle ops (listen/accept/send/recv/close and real spawn/wait/kill behavior).
- [x] P0.3 Replace non-production first-party crypto bridge primitives with production-grade algorithms/key-provider integration while preserving deterministic logs/replay contracts.

## Unresolved Backlog

- [ ] P1.1 Replace deterministic target wrapper artifacts with real deployment packagers for `ios`, `android`, `edge`, and `service-runtime` targets.
- [ ] P1.2 Remove remaining manual backend bootstrap debt outside workspace-scaffolded flows, including WASI remote registry/sync paths.
- [ ] P1.3 Expand first-party plugin/ffi bridge coverage from demo/limited ABI helpers to schema-driven general host ABI execution.
- [x] P2.1 Add a large-workspace agent-performance lane (>=10k module corpus) with enforced SLOs for `gcpm lock/build/test` and selfhost artifact refresh.

## Evidence Anchors

- `.genesis/perf/selfhost_readiness_report.json`
- `.genesis/perf/full_selfhost_cutover_profile_report.json`
- `.genesis/perf/agent_capability_gauntlet_release_confidence_report.json`
- `.genesis/perf/agent_generative_workloads_report.json`
- `.genesis/perf/agent_workflow_runtime_parity_report.json`
- `.genesis/perf/backend_starter_workflows_report.json`
- `.genesis/perf/domain_starter_registry_bootstrap_report.json`
- `.genesis/perf/gcpm_operation_contract_pack_report.json`
- `.genesis/perf/large_workspace_agent_perf_report.json`
- `.genesis/perf/large_workspace_agent_runtime_report.json`
- `.genesis/perf/hot_path_runtime_report.json`
- `.genesis/perf/upgrade_plan_health_profile_report.json`
- `crates/gc_opt/src/stage2_wasm.rs`
- `crates/gc_opt/src/stage2_wasm/pipeline_exec.rs`
- `crates/gc_cli_driver/src/host_bridge_runtime.rs`
- `crates/gc_cli_driver/src/pkg_workspace_ops_build_artifacts.rs`
- `docs/spec/HOST_ABI.md`
