# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-25

Scope:
- Track only unresolved upgrades required for AI-first authoring reliability, selfhost closure, and production runtime trust.
- Keep this file machine-syncable with `.genesis/perf/selfhost_readiness_report.json`, `docs/status/REDTEAM_REPORT.md`, and `feature_matrix.md`.
- Keep completed work out of this file (git history + perf artifacts are closure evidence).

Open checklist items: 1

## Critical Path

- [ ] P0.1 Close Stage2 coverage gaps that still reject valid CoreForm programs required by deploy targets.
  - Done when:
  - `crates/gc_opt/src/stage2_wasm/**/*.rs` no longer emits `Stage2CompileError::Unsupported` for recursive function calls, non-trivial collection pipelines, and supported higher-order patterns used by shipped domain workflows.
  - `genesis build --target edge|service-runtime` passes on expanded real-workload corpus without fallback-only lowering for semantically supported programs.
  - Translation-validation remains deterministic and replay-stable for all newly supported forms.
  - Progress landed (2026-02-26):
  - Stage2 recursive-call lowering now attempts deterministic compile-time fold to scalar when recursive arguments resolve to constants in scope, instead of hard-failing immediately at recursion detection.
  - Collection constant evaluators now handle nested `begin`/`let` alias pipelines for vector/map composition in strict lowering paths.
  - Added regression coverage for begin+let wrapped collection alias pipelines and removed stale recursion-specific error-message assertion.
  - Remaining blockers:
  - Add explicit fallback-mode accounting in Stage2 reports/CI gates so we can prove deploy-target workloads are not passing via module-level constant fallback.
  - Expand strict lowering coverage for supported higher-order callable-head patterns that still devolve to generic unsupported + fallback.
  - Run and archive expanded `edge`/`service-runtime` workload evidence showing strict translation-validation coverage at required corpus breadth.
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
- `crates/gc_effects/src/runner_gpu_host.rs`
- `crates/gc_effects/src/runner_gpu_device_backend.rs`
- `crates/gc_effects/src/runner_gfx_host.rs`
- `crates/gc_effects/src/runner_browser_host.rs`
- `crates/gc_effects/src/runner_xr_host.rs`
- `crates/gc_effects/src/runner_editor_tasks.rs`
- `crates/gc_effects/src/runner_editor_task_workflows.rs`
- `crates/gc_effects/src/runner_capability_dispatch/media.rs`
- `crates/gc_effects/src/runner_vcs_pkg_helpers/pkg_resolution.rs`
- `docs/spec/HOST_ABI.md`
- `docs/spec/EDITOR_CAPS.md`
- `docs/spec/CAPABILITY_COVERAGE_STATUS_v0.1.json`
- `docs/spec/CAPABILITY_COVERAGE_AUDIT_v0.1.json`
- `docs/spec/CAPABILITY_COVERAGE_AUDIT_v0.1.md`
- `scripts/check_capability_coverage_audit.sh`
