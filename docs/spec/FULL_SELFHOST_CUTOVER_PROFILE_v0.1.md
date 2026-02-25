> Bundle Entry: `docs/spec/HOST_RUNTIME_BUNDLE_v0.1.md`

# Full-Selfhost Cutover Profile v0.1

Status: normative release-candidate profile for proving the final selfhost boundary contract and closure path.

## Purpose

Define a single verification profile that proves GenesisCode is operating in selfhost-first mode,
with only explicit, bounded Rust host exceptions remaining.

This profile is intended to answer one question deterministically:

`What remains before the project can close the bootstrap era and treat Rust as non-semantic host glue only?`

## Remaining Exceptions (Explicit)

These are the only allowed semantic/runtime exceptions during v1 selfhost closure:

- `gc_coreform`
- `gc_kernel`
- `gc_prelude`
- `gc_effects`
- `gc_cli_driver`

No additional Rust semantic ownership is permitted without updating:

- `docs/spec/SELF_HOST_BOUNDARY.md`
- `docs/spec/FULL_SELFHOST_CUTOVER_PROFILE_v0.1.md`
- `scripts/check_full_selfhost_cutover_profile.sh`

## Exception Ownership + No-Semantic-Drift Proofs

Each explicit exception is allowed only while its ownership boundary and no-drift proof stays green.
The full-cutover gate requires all of the following reports to exist, match expected kinds, and be `ok=true`.

- `gc_coreform` + `gc_kernel` + `gc_prelude`:
  - `.genesis/perf/kernel_tcb_contract_report.json`
  - `.genesis/perf/selfhost_symbol_ownership_report.json`
- `gc_effects`:
  - `.genesis/perf/host_api_evolution_contract_report.json`
- `gc_cli_driver`:
  - `.genesis/perf/vcs_selfhost_contract_report.json`
  - `.genesis/perf/gcpm_operation_contract_pack_report.json`

If any proof report fails, the cutover profile fails closed.

## Closure Path

The closure path to a full selfhost posture is:

1. Keep production command routing selfhost-only (`docs/status/SELFHOST_CUTOVER.md` remains fully selfhost-routed).
2. Keep bootstrap retirement guard green (`scripts/check_bootstrap_retirement_gate.sh`).
3. Keep strict selfhost boundary guard green (`scripts/check_selfhost_boundary.sh --strict`).
4. Keep production rust frontend references at zero (`scripts/check_no_production_rust_frontend_refs.sh`).
5. Drive unresolved upgrade-plan blockers to zero and re-run this profile.

## Gate Contract

Primary gate:

- `scripts/check_full_selfhost_cutover_profile.sh`

This gate must verify:

1. This document contains explicit exceptions and closure steps.
2. `scripts/check_selfhost_boundary.sh --strict` remains compliant.
3. `scripts/check_bootstrap_retirement_gate.sh` report is valid and non-failing.
4. `scripts/check_selfhost_dashboard_fresh.sh` report is valid.
5. `scripts/check_selfhost_readiness_scorecard.sh` report is valid and only blocked by unresolved upgrade IDs, if any.
6. Exception proof reports are present and pass:
   - `scripts/check_kernel_tcb_contract.sh`
   - `scripts/check_host_api_evolution_contracts.sh`
   - `scripts/check_gcpm_operation_contract_pack.sh`
   - `scripts/check_vcs_selfhost_contract.sh`
   - `scripts/check_selfhost_symbol_ownership.sh`

## Health Profile Wiring

`scripts/check_upgrade_plan_health.sh` supports a dedicated profile:

- `--profile full-selfhost-cutover`

This profile runs the full-selfhost gate contract as a single lane.
