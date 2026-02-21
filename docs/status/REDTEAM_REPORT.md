# GenesisCode Red-Team Report

Last updated: 2026-02-21

Scope:
- Track unresolved `P0` and `P1` risks from `/Users/corbensorenson/Documents/genesisCode/upgrade_plan.md`.
- Keep entries actionable for self-hosted, AI-first v1 cutover.

## Active Risks (P0/P1)

- `P1.1` Selfhost artifact bootstrap latency is over budget (`check_perf_budgets.sh` regression).
- `P1.2` Host filesystem capability surface is incomplete for agent-authored project workflows (`io/fs::read|write` only).
- `P1.3` Process capability surface is one-shot only (`sys/process::exec`), lacking lifecycle/stream primitives.
- `P1.4` Host capability indices are operation-name oriented; per-op machine-readable payload/response schemas are missing.
