# GenesisCode Red-Team Report

Last updated: 2026-02-22

Scope:
- Track unresolved `P0` and `P1` risks from `/Users/corbensorenson/Documents/genesisCode/upgrade_plan.md`.
- Keep active risk IDs synchronized with `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/selfhost_readiness_report.json` (`unresolved_upgrade_plan_ids`).
- Keep entries actionable for self-hosted, AI-first v1 cutover.
- Reference machine-readable selfhost readiness source: `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/selfhost_readiness_report.json`.

## Active Risks (P0/P1)

- `P0.2`: Agent GPU/GFX workflows fail under constrained temp/disk conditions, blocking reliable AI iteration loops.
- `P0.3`: Target deployment pipeline remains runtime-runner contract based, not platform-native executable artifact based.
- `P1.2`: Tool qualification evidence accepts caller-provided test hashes without mandatory executed-run lineage binding.
- `P1.3`: `gcpm` dependency solver/range semantics are still local-only and below mature package-manager expectations.
- `P1.4`: Production CLI help-surface gate remains above budget and needs release-build reuse optimization.
- `P1.5`: Heavy gate suites need shared disk-headroom preflight/recovery to prevent infra-driven red runs.
- `P1.6`: High-churn Rust modules remain near threshold and still slow agent-first language evolution loops.
- `P1.7`: Documentation consolidation for agent-first authoring is incomplete and still fragmented across many markdown leaves.
- `P1.8`: Bootstrap retirement and fallback-removal enforcement is not yet fully closed for production-only pathways.
