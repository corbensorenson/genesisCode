# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-07-21

Scope:
- Track only unresolved upgrades required for AI-first authoring reliability, selfhost closure, and production runtime trust.
- This file is the canonical active P0/P1 defect-ID source. The capability ledger mirrors the exact IDs, and generated status views must match it.
- Keep completed work out of this file. Durable source history and E1-E4 evidence establish closure; mutable `.genesis/perf/` observations do not.

Open checklist items: 2

## Critical Path

- [ ] P1.4 Restore protected v0.5 publication and complete transitive generated/gate-input authority. The required `test_suite` fails GB-8 because `scripts/lib/genesisbench_mlx_custody.py` is an undeclared packaging module, proving the current v0.5 custody tranche is not publishable. Close only when R0.4.k governs the module and every recursively reached Rust/Python/helper input, all partial-freshness and parallel-workstream fan-in controls pass locally and in protected CI, one exact reviewed SHA is promoted to `main`, and the temporary branch is deleted without bypassing checks.
- [ ] P1.5 Eliminate host-bridge timeout kill/reap failure under mandatory fault injection. `runner_host_bridge::tests::spawn_per_op_timeout_kills_bridge_processes_and_recovers` produced `gpu/bridge-reap` after the process group survived repeated termination sweeps, so daemon and bridge cleanup are not yet reliable under the supported stress profile. Close only when R2.2.f proves success, failure, cancellation, timeout, restart, and repeated-load cleanup with bounded kill/reap latency and no surviving descendant on every supported native host.

## Evidence Anchors

- `upgrade_plan.md`
- `ROADMAP.md`
- `docs/spec/CAPABILITY_EVIDENCE_LEDGER_v0.1.json`
- `feature_matrix.md`
- `docs/status/REDTEAM_REPORT.md`
- `docs/status/SELFHOST_AUTHORITY_v0.1.md`
- `docs/spec/CAPABILITY_COVERAGE_STATUS_v0.1.json`
- `docs/spec/CAPABILITY_COVERAGE_AUDIT_v0.1.json`
- `docs/spec/CAPABILITY_COVERAGE_AUDIT_v0.1.md`

## Local Observation Inputs (E0, Not Closure Authority)

- `.genesis/perf/selfhost_readiness_report.json`
- `.genesis/perf/agent_capability_gauntlet_release_confidence_report.json`
- `.genesis/perf/agent_generative_workloads_report.json`
- `.genesis/perf/gcpm_operation_contract_pack_report.json`
- `.genesis/perf/remote_registry_runtime_parity_report.json`
- `.genesis/perf/gpu_device_conformance_report.json`
- `.genesis/perf/gpu_compute_runtime_profile_runtime_report.json`
- `.genesis/perf/gfx_runtime_profile_runtime_report.json`
- `.genesis/perf/webxr_browser_conformance_report.json`
- `.genesis/perf/gcpm_target_runtime_evidence_report.json`
- `.genesis/perf/source_decomposition_progress_report.json`
- `.genesis/perf/source_decomposition_tracked_parity_report.json`
- `.genesis/perf/ai_iteration_slo_metrics.json`
- `.genesis/perf/test_changed_fast_metrics.json`
