# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-25

Scope:
- Track only unresolved upgrades required for AI-first authoring reliability, selfhost closure, and productization trust.
- Keep this file machine-syncable with `.genesis/perf/selfhost_readiness_report.json` and `feature_matrix.md`.
- Keep completed work out of this file (git history + perf artifacts are closure evidence).

Open checklist items: 7

## Critical Path

- [ ] P0.1 Close the final Rust semantic exceptions and reach true selfhost semantic ownership.
  Why this is still open:
  - Full-cutover policy still explicitly allows semantic/runtime exceptions in `gc_coreform`, `gc_kernel`, `gc_prelude`, `gc_effects`, and `gc_cli_driver`.
  Evidence:
  - `docs/spec/FULL_SELFHOST_CUTOVER_PROFILE_v0.1.md:16`
  - `docs/spec/FULL_SELFHOST_CUTOVER_PROFILE_v0.1.md:20`
  - `.genesis/perf/full_selfhost_cutover_profile_report.json`
  Exit criteria:
  - `explicit_exceptions` in `.genesis/perf/full_selfhost_cutover_profile_report.json` is empty.
  - Cutover profile remains `ok=true` with no exception carve-outs.
  - `old_bootstrap/` remains archival-only and non-semantic.

- [ ] P0.2 Expand readiness scoring to include generative and cross-runtime parity truth, not just fixed critical gates.
  Why this is still open:
  - `selfhost-readiness` critical gate list currently omits `agent_generative_workloads` and `agent_workflow_runtime_parity`.
  Evidence:
  - `scripts/check_selfhost_readiness_scorecard.sh:381`
  - `.genesis/perf/agent_generative_workloads_report.json`
  - `.genesis/perf/agent_workflow_runtime_parity_report.json`
  Exit criteria:
  - Readiness critical gate contract includes and validates:
    - `.genesis/perf/agent_generative_workloads_report.json`
    - `.genesis/perf/agent_workflow_runtime_parity_report.json`
  - `selfhost_readiness_report.json` fails closed when either report is stale/failing.
  - Runtime parity p95 floor is raised from 1-sample enforcement to a stable minimum history floor.

- [ ] P1.1 Expand `gcpm` operation contract pack coverage from 5 operations to full automation-critical surface.
  Why this is still open:
  - The contract pack currently tracks only `{build, qualify, run, test, trace}`, while the command surface is much larger.
  Evidence:
  - `docs/spec/GCPM_OPERATION_CONTRACT_PACK_v0.1.json:11`
  - `.genesis/perf/gcpm_operation_contract_pack_report.json:15`
  - `docs/spec/GCPM_JSON_SCHEMAS_v0.1.md:29`
  Exit criteria:
  - Operation contract pack covers all gcpm commands used by autonomous workflows (`new`, `scaffold`, `add/remove`, `lock/update`, `install/verify`, `publish/sync`, `env/build`, `abi`, `doctor`, `self-optimize`, assurance ops).
  - Drift gate compares pack coverage directly against CLI schema IDs and fails on gaps.

- [ ] P1.2 Promote strict device-backed GPU truth in default agent confidence lanes; isolate fallback lanes as non-release evidence.
  Why this is still open:
  - Current gauntlet allows fallback-backed success (`require_gpu_device_backend=false`, multiple workflows on `deterministic-fallback`).
  Evidence:
  - `.genesis/perf/agent_capability_gauntlet_report.json:141`
  - `.genesis/perf/agent_capability_gauntlet_report.json:201`
  - `.genesis/perf/agent_capability_gauntlet_report.json:533`
  - `.genesis/perf/agent_capability_gauntlet_report.json:616`
  Exit criteria:
  - Default release-confidence gauntlet profile requires device runtime for GPU domains.
  - Fallback mode remains available but clearly separated as dev-only evidence.
  - Readiness and feature evidence consume strict-device lane for productization claims.

- [ ] P1.3 Tighten hot-path runtime gate quality (budget realism, history depth, and compile-vs-measure separation).
  Why this is still open:
  - Runtime profile allows very high elapsed budget and currently passes with low history depth despite large jitter.
  Evidence:
  - `.genesis/perf/hot_path_runtime_report.json:4`
  - `.genesis/perf/hot_path_runtime_report.json:7`
  - `.genesis/perf/hot_path_runtime_report.json:10`
  - `scripts/check_hot_path_budgets.sh:37`
  - `scripts/check_hot_path_budgets.sh:73`
  Exit criteria:
  - Runtime budget is reduced to an AI-iteration-appropriate envelope.
  - Cold compile/setup time is reported separately from hot-path measurement time.
  - History floor and regression checks fail closed on insufficient sample depth.

- [ ] P2.1 Expand deterministic `fix_options`/remediation reports beyond `gcpm lock|update|publish`.
  Why this is still open:
  - Workflow report contract documents deterministic remediation only for three commands.
  Evidence:
  - `docs/spec/GCPM_WORKFLOW_REPORTS_v0.1.md:8`
  - `docs/spec/GCPM_WORKFLOW_REPORTS_v0.1.md:12`
  Exit criteria:
  - `gcpm add/remove/install/verify/build/run/doctor/env/self-optimize` emit machine-actionable remediation plans with stable IDs.
  - Agents can continue autonomously after common failures without heuristic prompt repair.

- [ ] P2.2 Reduce default health/profile wall-time for faster agent inner-loop iteration.
  Why this is still open:
  - Current dev-fast profile still runs near multi-minute wall time.
  Evidence:
  - `.genesis/perf/upgrade_plan_health_profile_report.json`
  - `.genesis/perf/agent_capability_gauntlet_report.json`
  Exit criteria:
  - Default dev-fast health lane hits a tighter wall budget with deterministic pass rates.
  - Inner-loop profile remains coverage-complete for AI authoring guardrails while reducing latency.

## Evidence Anchors

- `.genesis/perf/selfhost_readiness_report.json`
- `.genesis/perf/full_selfhost_cutover_profile_report.json`
- `.genesis/perf/agent_capability_gauntlet_report.json`
- `.genesis/perf/agent_generative_workloads_report.json`
- `.genesis/perf/agent_workflow_runtime_parity_report.json`
- `.genesis/perf/gcpm_operation_contract_pack_report.json`
- `.genesis/perf/hot_path_runtime_report.json`
- `.genesis/perf/upgrade_plan_health_profile_report.json`
