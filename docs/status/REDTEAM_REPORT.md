# GenesisCode Red-Team Report

Last updated: 2026-02-22

Scope:
- Track unresolved `P0` and `P1` risks from `/Users/corbensorenson/Documents/genesisCode/upgrade_plan.md`.
- Keep entries actionable for self-hosted, AI-first v1 cutover.
- Reference machine-readable selfhost readiness source: `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/selfhost_readiness_report.json`.

## Active Risks (P0/P1)

- `P0.1` Planning truthfulness drift (partial capability rows + zero-gap reporting mismatch).
- `P0.2` `gcpm build` target support remains metadata-only for mobile/edge/service-runtime.
- `P1.1` Missing executable conformance workflows for `ios|android|edge|service-runtime`.
- `P1.2` Local AI authoring loop lacks bounded deterministic fast profile.
- `P1.3` SLO schema/budget enforcement is not normalized across major gauntlet/parity reports.
- `P1.4` Oversized high-churn Rust modules still impede AI-maintainable iteration.
- `P1.7` Semantic-edit workflow lacks first-class deterministic apply execution.
