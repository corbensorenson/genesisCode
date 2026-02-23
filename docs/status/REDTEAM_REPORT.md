# GenesisCode Red-Team Report

Last updated: 2026-02-22

Scope:
- Track unresolved `P0` and `P1` risks from `/Users/corbensorenson/Documents/genesisCode/upgrade_plan.md`.
- Keep active risk IDs synchronized with `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/selfhost_readiness_report.json` (`unresolved_upgrade_plan_ids`).
- Keep entries actionable for self-hosted, AI-first v1 cutover.
- Reference machine-readable selfhost readiness source: `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/selfhost_readiness_report.json`.

## Active Risks (P0/P1)

- `P1.3`: `gcpm` dependency solver/range semantics are improved but still below mature package-manager expectations (semver tag-range solving exists; registry conflict diagnostics and full workspace selective-upgrade ergonomics remain open).
