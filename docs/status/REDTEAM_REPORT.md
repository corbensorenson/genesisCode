# GenesisCode Red-Team Report

Last updated: 2026-02-23

Scope:
- Track unresolved `P0`/`P1` risks from `/Users/corbensorenson/Documents/genesisCode/upgrade_plan.md`.
- Keep active IDs synchronized with `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/selfhost_readiness_report.json`.
- Focus on blockers to self-hosted, fully functional, AI-first operation.

## Active Risks (P0/P1)

- P0.1 - prepush profile runtime is too slow for AI inner-loop iteration.
- P1.1 - top Rust production modules remain near the decomposition cap.
- P1.2 - largest selfhost/prelude `.gc` modules remain too large for optimal AI edit locality.
