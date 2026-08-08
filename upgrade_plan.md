# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-08-08

Scope:
- Track only unresolved upgrades required for AI-first authoring reliability, selfhost closure, and production runtime trust.
- This file is the canonical active P0/P1 defect-ID source. The capability ledger mirrors the exact IDs, and generated status views must match it.
- Keep completed work out of this file. Durable source history and E1-E4 evidence establish closure; mutable `.genesis/perf/` observations do not.

Open checklist items: 3

## Critical Path

- [ ] P1.5 Eliminate host-bridge termination failure and error suppression under mandatory fault injection. `runner_host_bridge::tests::spawn_per_op_timeout_kills_bridge_processes_and_recovers` produced `gpu/bridge-reap` after the process group survived repeated termination sweeps, while persistent-session disconnect, malformed-response cleanup, and owner teardown can discard `stop()` failures before they reach the sealed operation boundary. Close only when R2.2.f proves success, malformed response, disconnection, failure, cancellation, timeout, owner drop, daemon restart, and repeated-load cleanup through public operations; every cleanup failure must remain observable as a typed reap/cleanup error, all workers must join or remain under an explicitly reported bounded containment failure, and no descendant may survive on any supported native host.
- [ ] P1.6 Restore independently watched exact-main standard-profile disposition and truthful R0.4.j closure. The watchdog classifies exact-head `push` runs, which are `fast`, and full dispatches, but does not classify or require the separately dispatched `standard` profile promised by R0.4.j and M0. Exact revision `6d22bc1e1eebee6ac76b8c20fb513f7d14b37d5b` demonstrated the counterexample: standard run `31269647811` failed while full and multiple watchdog runs passed. Close only when policy and evaluator distinguish standard from fast/full, require the latest exact-main standard run to reach a successful terminal disposition within two hours, reject missing, stale, cancelled, failed, wrong-head, and superseded-only evidence, preserve append-only history, and independently recognize a passing exact-main standard/full pair.
- [ ] P1.7 Restore fail-closed aggregate resource ownership for generated-authority publication and its transitive-input closure. Generated validation sets `GENESIS_GATE_BUDGET_ENFORCE=0` for every nested check, disabling duration, generated-disk, and deny-network enforcement, while the orchestrator enforces only broad node timeouts and validates declared `diskMiB` values without measuring them. This is a post-closure regression against the completed R0.4.i/R0.4.k contracts and is corrected through reopened R0.4.j without erasing their historical evidence. Close only when nested checks retain hard duration and deny-network enforcement, shared parallel disk attribution is disabled only under an actual aggregate observer, the complete transaction enforces declared wall/disk bounds and kills/reaps every child on failure, ordinary callers cannot select a passing observation-only mode, the complete R0.4.i/R0.4.k guard set re-proves transitive routing under the corrected supervisor, and mutation controls prove duration, disk, network, caller-environment, missing-aggregate-owner, and partial-publication failures.

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
