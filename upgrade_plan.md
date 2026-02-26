# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-25

Scope:
- Track only unresolved upgrades required for AI-first authoring reliability, selfhost closure, and production runtime trust.
- Keep this file machine-syncable with `.genesis/perf/selfhost_readiness_report.json`, `docs/status/REDTEAM_REPORT.md`, and `feature_matrix.md`.
- Keep completed work out of this file (git history + perf artifacts are closure evidence).

Open checklist items: 7

## Critical Path

- [ ] P0.1 Close Stage2 coverage gaps that still reject valid CoreForm programs required by deploy targets.
  - Done when:
  - `crates/gc_opt/src/stage2_wasm/**/*.rs` no longer emits `Stage2CompileError::Unsupported` for recursive function calls, non-trivial collection pipelines, and supported higher-order patterns used by shipped domain workflows.
  - `genesis build --target edge|service-runtime` passes on expanded real-workload corpus without fallback-only lowering for semantically supported programs.
  - Translation-validation remains deterministic and replay-stable for all newly supported forms.
- [ ] P0.2 Deliver full WASI remote registry parity (http/https remotes) without `wasi_http_unsupported` hard failures.
  - Done when:
  - `crates/gc_registry/src/registry/client_impl/{ping_and_store,refs}.rs` support `ping`, `store/*`, and `refs/*` in WASI through deterministic bridge transport.
  - Auth, chunk upload, and body/resource limits remain policy-gated and replay-safe.
  - `gcpm add/install/lock/publish/sync` behave equivalently across native and WASI for networked registries.
- [ ] P0.3 Replace first-party GPU placeholder semantics with production-capable device-runtime parity.
  - Done when:
  - `crates/gc_effects/src/runner_gpu_device_backend.rs` supports resource lifecycle ops (`create-*`, `write-*`, `read-*`, `destroy-resource`) and not only submit/limits/features.
  - `crates/gc_effects/src/runner_gpu_host.rs` no longer relies on opaque-hash placeholders for pipelines in production profiles.
  - Deterministic replay evidence exists for GPU compute + gfx interop flows, not just synthetic hashes.
- [ ] P0.4 Close host runtime realism gap for gfx/browser/xr in production profiles.
  - Done when:
  - `crates/gc_effects/src/runner_gfx_host.rs`, `runner_browser_host.rs`, and `runner_xr_host.rs` default production profile paths exercise real adapters (not headless/noop simulation) for lifecycle-critical ops.
  - Unsupported-op surfaces are either implemented or explicitly policy-disabled with schema-level declarations and conformance tests.
  - Browser/XR/GFX productization kits pass with device-backed evidence artifacts.

## High Priority

- [ ] P1.1 Expand media capability coverage beyond current narrow conversion matrix.
  - Done when:
  - `crates/gc_effects/src/runner_capability_dispatch/media.rs` supports policy-gated mainstream image/audio format families and explicit deterministic conversion pipelines.
  - Unsupported conversion failures are reduced to truly out-of-scope formats, not common production pathways.
- [ ] P1.2 Expand first-party editor task runtime from fixed task set to extensible AI workflow orchestration.
  - Done when:
  - `crates/gc_effects/src/runner_editor_tasks.rs` supports schema-driven task kinds with stable contracts for build/run/debug/refactor/index operations.
  - Task spawn/poll/cancel semantics support long-running incremental workflows and structured partial outputs.

## Agent Productization

- [ ] P2.3 Strengthen untrusted-agent execution safety for “build anything” operation mode.
  - Done when:
  - Resource quotas, sandbox boundaries, and effect-policy defaults are enforced for generated code execution in local/dev/CI profiles.
  - Security posture is validated by repeatable abuse-case tests (resource exhaustion, host escape attempts, capability abuse).

## Evidence Anchors

- `upgrade_plan.md`
- `feature_matrix.md`
- `docs/status/REDTEAM_REPORT.md`
- `.genesis/perf/selfhost_readiness_report.json`
- `.genesis/perf/full_selfhost_cutover_profile_report.json`
- `.genesis/perf/agent_capability_gauntlet_release_confidence_report.json`
- `.genesis/perf/agent_generative_workloads_report.json`
- `.genesis/perf/large_workspace_agent_perf_report.json`
- `.genesis/perf/upgrade_plan_health_profile_report.json`
- `.genesis/perf/upgrade_plan_health_release_full_history.jsonl`
- `crates/gc_opt/src/stage2_wasm.rs`
- `crates/gc_opt/src/stage2_wasm/pipeline_exec.rs`
- `crates/gc_registry/src/registry/client_impl/ping_and_store.rs`
- `crates/gc_registry/src/registry/client_impl/refs.rs`
- `crates/gc_effects/src/runner_gpu_host.rs`
- `crates/gc_effects/src/runner_gpu_device_backend.rs`
- `crates/gc_effects/src/runner_gfx_host.rs`
- `crates/gc_effects/src/runner_browser_host.rs`
- `crates/gc_effects/src/runner_xr_host.rs`
- `crates/gc_effects/src/runner_editor_tasks.rs`
- `crates/gc_effects/src/runner_capability_dispatch/media.rs`
- `crates/gc_effects/src/runner_vcs_pkg_helpers/pkg_resolution.rs`
- `docs/spec/HOST_ABI.md`
- `docs/spec/CAPABILITY_COVERAGE_STATUS_v0.1.json`
- `docs/spec/CAPABILITY_COVERAGE_AUDIT_v0.1.json`
- `docs/spec/CAPABILITY_COVERAGE_AUDIT_v0.1.md`
- `scripts/check_capability_coverage_audit.sh`
