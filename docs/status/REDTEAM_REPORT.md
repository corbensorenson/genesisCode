# GenesisCode Red-Team Report

Last updated: 2026-02-22

Scope:
- Track unresolved `P0` and `P1` risks from `/Users/corbensorenson/Documents/genesisCode/upgrade_plan.md`.
- Keep entries actionable for self-hosted, AI-first v1 cutover.

## Active Risks (P0/P1)

- P0.1 - Artifact bootstrap deadlock: production selfhost artifact regeneration requires an existing artifact seed.
- P1.5 - GPU compute backend policy defaults permit fallback in lanes that should be strict-device.
- P1.6 - Agent authoring skill gates are mostly structural; executable quality conformance is missing.
