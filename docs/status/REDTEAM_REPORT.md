# GenesisCode Red-Team Report

Last updated: 2026-02-22

Scope:
- Track unresolved `P0` and `P1` risks from `/Users/corbensorenson/Documents/genesisCode/upgrade_plan.md`.
- Keep entries actionable for self-hosted, AI-first v1 cutover.

## Active Risks (P0/P1)

- `P0.1` - `check_upgrade_plan_health.sh` can crash on Bash 3 (`set -u` empty-array expansion), blocking reliable health gating.
- `P0.2` - release guard path does not fail fast under low disk and can attempt to execute corrupted/non-executable artifacts.
- `P1.1` - `selfhost/toolchain.gc` freshness gate is red; committed artifact is stale relative to source.
- `P1.2` - runtime backend feature matrix lane is disk-fragile and failed with `No space left on device`.
- `P1.3` - compile-heavy script fleet lacks consistent cargo target-dir policy, driving cache sprawl and iteration slowdown.
- `P1.4` - compile-heavy non-health lanes lack standardized timing telemetry, limiting optimization and regression tracking.
