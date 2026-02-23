# GenesisCode Red-Team Report

Last updated: 2026-02-23

Scope:
- Track unresolved `P0` and `P1` risks from `/Users/corbensorenson/Documents/genesisCode/upgrade_plan.md`.
- Keep active risk IDs synchronized with `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/selfhost_readiness_report.json` (`unresolved_upgrade_plan_ids`).
- Keep entries actionable for self-hosted, AI-first v1 cutover.
- Reference machine-readable selfhost readiness source: `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/selfhost_readiness_report.json`.

## Active Risks (P0/P1)

- `P0.4` - Strict health profiles (`prepush-standard`, `release-full`) currently non-green from clean execution.
- `P1.1` - WebXR conformance lane is deterministic but functionally degraded (`frame timeout`, `session_close error`).
- `P1.2` - Documentation complexity budget has no headroom for agent retrieval quality.
- `P1.3` - High-churn assurance/runtime surfaces need decomposition for AI maintainability.
- `P1.4` - Strict profile warmup latency is high for agent inner loops.
- `P1.5` - Residual parity-harness ownership dependencies keep full selfhost closure partial.
