# GenesisCode Red-Team Report

Last updated: 2026-02-21

Scope:
- Track unresolved `P0` and `P1` risks from `/Users/corbensorenson/Documents/genesisCode/upgrade_plan.md`.
- Keep entries actionable for self-hosted, AI-first v1 cutover.

## Active Risks (P0/P1)

- `P1.1` Agent workflow validation remains smoke-oriented; no scored multi-domain capability gauntlet is enforced.
- `P1.2` Test/iteration profile budgets still allow high-latency loops for agent development (`changed-fast` 5m, `full-cross-host` 20m).
- `P1.3` Documentation/spec retrieval surface remains large; bundle-first consolidation and orphan-guarding are incomplete.
- `P1.4` `.agents/skills/genesiscode-authoring/SKILL.md` lacks CI drift guards against current schema/capability contracts.
