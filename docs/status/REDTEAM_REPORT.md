# GenesisCode Red-Team Report

Last updated: 2026-02-22

Scope:
- Track unresolved `P0` and `P1` risks from `/Users/corbensorenson/Documents/genesisCode/upgrade_plan.md`.
- Keep entries actionable for self-hosted, AI-first v1 cutover.

## Active Risks (P0/P1)

- P0.1 - Artifact bootstrap deadlock: production selfhost artifact regeneration requires an existing artifact seed.
- P1.1 - `dev-fast` profile wall time is too high for tight agent iteration loops.
- P1.2 - Build cache growth still causes ENOSPC risk under repeated gate execution.
- P1.3 - Runtime backend feature matrix check has no enforced wall-time SLO/budget.
- P1.4 - Production CLI parse/help surface checks are still high-latency release runs.
- P1.5 - GPU compute backend policy defaults permit fallback in lanes that should be strict-device.
- P1.6 - Agent authoring skill gates are mostly structural; executable quality conformance is missing.
- P1.7 - Feature-matrix bootstrap-independence claim is ahead of current recovery reality.
