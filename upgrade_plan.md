# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-25

Scope:
- Track only unresolved upgrades required for AI-first authoring reliability, selfhost closure, and productization trust.
- Keep this file machine-syncable with `.genesis/perf/selfhost_readiness_report.json` and `feature_matrix.md`.
- Keep completed work out of this file (git history + perf artifacts are closure evidence).

Open checklist items: 1

## Critical Path

- P1.3 - external ecosystem bridge into GenesisPkg

## Unresolved Backlog

- [ ] P1.3 External ecosystem bridge into GenesisPkg
Why: language-native package model is strong, but "build anything" adoption still needs deterministic mirror/bridge flows for external ecosystems (e.g., crates/npm/pypi) into GenesisPkg artifacts with provenance.
Done when:
  - mirrored external packages are transformed into signed GenesisPkg snapshots/commits.
  - dependency policy can pin mirrored provenance roots and replay conversion evidence.
  - bridge operations are capability-gated and auditable.
Evidence:
  - `docs/spec/GCPM_JSON_SCHEMAS_v0.1.md`
  - `.genesis/perf/gcpm_operation_contract_pack_report.json`

## Evidence Anchors

- `.genesis/perf/selfhost_readiness_report.json`
- `.genesis/perf/full_selfhost_cutover_profile_report.json`
- `.genesis/perf/agent_capability_gauntlet_release_confidence_report.json`
- `.genesis/perf/agent_generative_workloads_report.json`
- `.genesis/perf/agent_workflow_runtime_parity_report.json`
- `.genesis/perf/gcpm_operation_contract_pack_report.json`
- `.genesis/perf/hot_path_runtime_report.json`
- `.genesis/perf/upgrade_plan_health_profile_report.json`
