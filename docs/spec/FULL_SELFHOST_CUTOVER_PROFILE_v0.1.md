> Bundle Entry: `docs/spec/HOST_RUNTIME_BUNDLE_v0.1.md`

# Full-Selfhost Cutover Profile v0.1

Status: normative release-candidate profile for proving the final selfhost boundary contract and closure path.

## Purpose

Define a single verification profile that proves GenesisCode is operating in selfhost-first mode
with zero explicit Rust semantic/runtime exception carve-outs.

This profile is intended to answer one question deterministically:

`What remains before the project can close the bootstrap era and treat Rust as non-semantic host glue only?`

## Remaining Exceptions (Explicit)

- none

## Exception Ownership + No-Semantic-Drift Proofs

The no-exception closure contract is only valid while ownership boundary and no-drift proofs stay green.
The full-cutover gate requires all of the following reports to exist, match expected kinds, and be `ok=true`.

- kernel TCB ownership (`gc_coreform` + `gc_kernel` + `gc_prelude`):
  - `.genesis/perf/kernel_tcb_contract_report.json`
    - explicit producer: `bash scripts/update_kernel_tcb_contract_report.sh`
  - `.genesis/perf/selfhost_symbol_ownership_report.json`
    - explicit producer: `bash scripts/update_selfhost_symbol_ownership_report.sh`
- host capability/runtime bridge (`gc_effects`):
  - `.genesis/perf/host_api_evolution_contract_report.json`
    - explicit producer: `bash scripts/update_host_api_evolution_contract_report.sh`
- operation/driver contract stability (`gc_cli_driver`):
  - `.genesis/perf/vcs_selfhost_contract_report.json`
    - explicit producer: `bash scripts/update_vcs_selfhost_contract_report.sh`
  - `.genesis/perf/gcpm_operation_contract_pack_report.json`
    - explicit producer: `bash scripts/update_gcpm_operation_contract_pack_report.sh`

If any proof report fails, the cutover profile fails closed.

## Closure Path

The closure path to a full selfhost posture is:

1. Keep production command routing selfhost-only (`docs/status/SELFHOST_CUTOVER.md` remains fully selfhost-routed).
2. Keep bootstrap retirement guard green (`scripts/check_bootstrap_retirement_gate.sh`).
   - retained E0 producer: `bash scripts/update_bootstrap_retirement_gate_report.sh`
3. Keep strict selfhost boundary guard green (`scripts/check_selfhost_boundary.sh --strict`).
4. Keep production rust frontend references at zero (`scripts/check_no_production_rust_frontend_refs.sh`).
5. Drive unresolved upgrade-plan blockers to zero and re-run this profile.

## Gate Contract

Primary gate:

- `scripts/check_full_selfhost_cutover_profile.sh`

The gate is read-only and renders its result into private temporary paths. To retain
the aggregate E0 report and history after all prerequisite evidence is current, run:

- `bash scripts/update_full_selfhost_cutover_profile_report.sh`

This gate must verify:

1. This document contains explicit exceptions and closure steps.
2. `scripts/check_selfhost_boundary.sh --strict` remains compliant.
3. The bootstrap retirement report produced by
   `scripts/update_bootstrap_retirement_gate_report.sh` is valid and non-failing.
4. `scripts/check_selfhost_dashboard_fresh.sh` report is valid.
   - explicit producer: `bash scripts/update_selfhost_dashboard_fresh_report.sh`
5. The readiness report produced by
   `scripts/update_selfhost_readiness_scorecard_report.sh` is valid and only blocked by unresolved upgrade IDs, if any.
6. Exception proof reports are present and pass:
   - `scripts/check_kernel_tcb_contract.sh`
   - `scripts/check_host_api_evolution_contracts.sh`
   - `scripts/check_gcpm_operation_contract_pack.sh`
   - `scripts/check_vcs_selfhost_contract.sh`
   - `scripts/check_selfhost_symbol_ownership.sh`
7. Stage2 minima in `selfhost/toolchain.gc` are policy-enforced against refreshed generative corpus coverage:
   - `.genesis/perf/agent_generative_workloads_report.json` must be `ok=true`
   - explicit producer: `bash scripts/update_agent_generative_workloads_report.sh`
   - `:stage2-requirements/:min-supported-modules` and `:min-validated-modules` must be at least `required_domains + 2`.

## Health Profile Wiring

`scripts/check_upgrade_plan_health.sh` supports a dedicated profile:

- `--profile full-selfhost-cutover`

This profile runs the full-selfhost gate contract as a single lane.
