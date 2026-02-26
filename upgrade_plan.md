# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-25

Scope:
- Track only unresolved upgrades required for AI-first authoring reliability, selfhost closure, and production runtime trust.
- Keep this file machine-syncable with `.genesis/perf/selfhost_readiness_report.json`, `docs/status/REDTEAM_REPORT.md`, and `feature_matrix.md`.
- Keep completed work out of this file (git history + perf artifacts are closure evidence).

Open checklist items: 4

## Critical Path

- [ ] P0.2 Close remote-registry parity gaps for `core/store`, `core/sync`, and `core/pkg-low` across native and WASI runtimes.
  - Why this blocks "agent can build anything":
  - Agents cannot reliably publish/install/sync packages in all deployment profiles until remote registry behavior is equivalent and deterministic in both runtime lanes.
  - Done when:
  - Capability families `core/store`, `core/sync`, and `core/pkg-low` move from `planned` to `implemented` in `docs/spec/CAPABILITY_COVERAGE_STATUS_v0.1.json`.
  - `scripts/check_gcpm_operation_contract_pack.sh` and `scripts/check_agent_reference_workflows.sh` pass with explicit native + WASI remote-registry scenarios (import/export/push/pull/chunk upload/ref update).
  - `docs/spec/CAPABILITY_COVERAGE_AUDIT_v0.1.json` no longer reports `P0.2` planned families.

- [ ] P0.3 Retire remaining GPU placeholder semantics and enforce real device-backed compute contracts.
  - Why this blocks "agent can build anything":
  - GPU-heavy workloads (simulation, rendering, ML-style compute kernels) require device-backed semantics, not placeholder behavior.
  - Done when:
  - Capability families `gpu/compute` and `gfx/gpu` move from `planned` to `implemented` in `docs/spec/CAPABILITY_COVERAGE_STATUS_v0.1.json`.
  - `scripts/check_gpu_compute_runtime_profile.sh`, `scripts/check_gpu_device_conformance_matrix.sh`, and `scripts/check_gpu_stack_decoupling.sh` all pass on required device lanes.
  - `.genesis/perf/gpu_device_conformance_report.json` and lane reports show `ok: true` with no placeholder-mode escape paths.

- [ ] P0.4 Ship production-real browser/gfx/xr capability families (window/input/audio/time/storage/xr), not just planned surface.
  - Why this blocks "agent can build anything":
  - Interactive application classes (games, realtime editors, browser-hosted tools, XR) are still marked planned in capability coverage.
  - Done when:
  - Families `browser/window`, `browser/input`, `browser/audio`, `browser/storage`, `gfx/window`, `gfx/input`, `gfx/audio`, `gfx/time`, and `gfx/xr` move to `implemented`.
  - `scripts/check_gfx_runtime_profile.sh`, `scripts/check_webxr_browser_conformance_lane.sh`, and `scripts/check_gpu_xr_productization_kits.sh` pass in release lanes.
  - Capability audit no longer reports `P0.4` planned families.

- [ ] P0.5 Introduce a safe, policy-audited FFI escalation path for advanced agent workloads while preserving deny-by-default defaults.
  - Why this blocks "agent can build anything":
  - `host/ffi` is currently policy-disabled, which blocks integration with ecosystems/drivers/libraries outside the first-party bridge surface.
  - Done when:
  - `host/ffi` has a documented signed-policy enablement path with explicit quotas, syscall boundaries, and deterministic effect/evidence logging.
  - `scripts/check_host_abi_conformance.sh` includes enabled-FFI policy profile tests plus adversarial abuse-case tests.
  - Default profile remains deny-by-default; release profile requires explicit policy artifact opt-in.

## Evidence Anchors

- `upgrade_plan.md`
- `feature_matrix.md`
- `docs/status/REDTEAM_REPORT.md`
- `docs/spec/CAPABILITY_COVERAGE_STATUS_v0.1.json`
- `docs/spec/CAPABILITY_COVERAGE_AUDIT_v0.1.json`
- `docs/spec/CAPABILITY_COVERAGE_AUDIT_v0.1.md`
- `.genesis/perf/selfhost_readiness_report.json`
- `.genesis/perf/agent_capability_gauntlet_release_confidence_report.json`
- `.genesis/perf/agent_generative_workloads_report.json`
- `.genesis/perf/gcpm_operation_contract_pack_report.json`
- `.genesis/perf/gpu_device_conformance_report.json`
- `.genesis/perf/gpu_compute_runtime_profile_runtime_report.json`
- `.genesis/perf/gfx_runtime_profile_runtime_report.json`
- `.genesis/perf/webxr_browser_conformance_report.json`
- `.genesis/perf/source_decomposition_progress_report.json`
- `.genesis/perf/ai_iteration_slo_metrics.json`
- `.genesis/perf/test_changed_fast_metrics.json`
