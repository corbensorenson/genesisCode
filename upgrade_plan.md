# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-26

Scope:
- Track only unresolved upgrades required for AI-first authoring reliability, selfhost closure, and production runtime trust.
- Keep this file machine-syncable with `.genesis/perf/selfhost_readiness_report.json`, `docs/status/REDTEAM_REPORT.md`, and `feature_matrix.md`.
- Keep completed work out of this file (git history + perf artifacts are closure evidence).

Open checklist items: 6

## Critical Path

- [ ] P0.1 Make critical readiness artifacts self-refreshing and non-stale by default.
  - Why this blocks "agent can build anything":
  - Agent trust in release readiness is currently undermined by stale critical artifacts.
  - Current evidence:
  - `.genesis/perf/selfhost_readiness_report.json` has `critical_gate_truth.ok=false` with stale freshness errors for `agent_workflow_runtime_parity` and `gpu_gfx_headroom_conformance`.
  - Done when:
  - `scripts/check_selfhost_readiness_scorecard.sh` (default env) produces `critical_gate_truth.ok=true`.
  - Stale critical artifacts always refresh (or hard-fail) when readiness is evaluated.
  - `refresh_skipped` is absent from stale critical checks in readiness output when refresh is not explicitly disabled.

- [ ] P0.2 Bound agent workflow parity refresh latency so freshness checks stay reliable.
  - Why this blocks "agent can build anything":
  - Readiness cannot be a trustworthy control plane if parity evidence is frequently stale because refresh is too heavy for routine execution.
  - Current evidence:
  - `.genesis/perf/selfhost_readiness_report.json` reports `agent-workflow-runtime-parity:freshness-stale`.
  - Full refresh path for parity currently fans out dual `check_agent_reference_workflows.sh` lanes and is expensive in local iteration loops.
  - Done when:
  - `.genesis/perf/agent_workflow_runtime_parity_report.json` is refreshed inside TTL during default readiness runs.
  - Parity refresh path has an enforced runtime SLO and history-backed regression guard.
  - Readiness critical gate truth no longer needs manual freshness overrides for parity artifacts.

- [ ] P1.1 Upgrade GPU/GFX headroom conformance from fallback-only to mixed device-required coverage.
  - Why this matters:
  - AI-generated GPU workloads need verified behavior both on real device backends and fallback lanes under pressure.
  - Current evidence:
  - `.genesis/perf/gpu_gfx_headroom_conformance_report.json` replays fallback backend (`deterministic-fallback`) in both normal and low-headroom lanes.
  - Done when:
  - Headroom conformance includes a `require-device` lane that asserts `device-runtime` backend when device runtime is available.
  - Low-headroom lane may fallback but must emit explicit policy/evidence for fallback.
  - Report schema captures backend mode per lane and readiness consumes it.

- [ ] P1.2 Productize a safe FFI escalation path (while keeping default deny-by-default).
  - Why this blocks "agent can build anything":
  - Agent tasks requiring novel external libraries/drivers are capped while `host/ffi` remains policy-disabled only.
  - Current evidence:
  - `docs/spec/CAPABILITY_COVERAGE_STATUS_v0.1.json` marks `host/ffi` as `policy-disabled`.
  - Done when:
  - Signed FFI policy profile exists with syscall/library allowlists, quotas, and audit requirements.
  - `scripts/check_host_abi_conformance.sh` covers enabled-FFI profiles plus abuse-case denial tests.
  - FFI usage is provenance-linked in effect/evidence logs.

- [ ] P1.3 Burn down source decomposition debt on tracked over-budget modules.
  - Why this blocks AI-first iteration velocity:
  - Very large modules reduce locality, increase agent edit risk, and slow multi-agent parallelism.
  - Current evidence:
  - `.genesis/perf/source_decomposition_progress_report.json` tracks 9 over-budget modules (up to 1741 lines), all still `planned`.
  - Done when:
  - All tracked over-budget modules are split to target size (<=700) or formally waived with bounded ownership rationale.
  - Every listed parity gate in decomposition policy passes after split.
  - Coverage modules and ownership docs are updated to reflect new boundaries.

- [ ] P1.4 Add non-synthetic deployment runtime validation for target pipelines.
  - Why this blocks "agent can build anything":
  - Current target pipeline validation is deterministic and reproducible but still adapter/smoke-script centric.
  - Current evidence:
  - `scripts/check_gcpm_target_runtime_pipelines.sh` validates deterministic artifacts and launch adapters, but not real-device/emulator/runtime execution evidence per target.
  - Done when:
  - CI lanes provide target-class runtime evidence for iOS/Android/edge/service-runtime (emulator/device/container as applicable).
  - Evidence is attached as replayable artifacts and consumed by readiness critical gate truth.
  - Target runtime failures surface as first-class upgrade blockers.

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
- `.genesis/perf/remote_registry_runtime_parity_report.json`
- `.genesis/perf/gpu_device_conformance_report.json`
- `.genesis/perf/gpu_compute_runtime_profile_runtime_report.json`
- `.genesis/perf/gfx_runtime_profile_runtime_report.json`
- `.genesis/perf/webxr_browser_conformance_report.json`
- `.genesis/perf/source_decomposition_progress_report.json`
- `.genesis/perf/ai_iteration_slo_metrics.json`
- `.genesis/perf/test_changed_fast_metrics.json`
