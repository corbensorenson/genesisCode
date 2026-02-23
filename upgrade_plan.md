# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-23

This file contains only open items from the latest red-team pass.
Completed work must be removed from this file and kept in git history/release notes.
Machine-readable selfhost readiness source: `.genesis/perf/selfhost_readiness_report.json`.

Open checklist items: 1

## P0 - Self-Host and Iteration Blockers

- [ ] P0.1 Cut strict prepush lane wall-time to AI-usable latency.
  Evidence:
  `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/upgrade_plan_health_profile_report.json` shows:
  `elapsed_ms = 737428`, `profile = "prepush-standard"`, `gate_count = 67`.
  This is ~12.3 minutes, too slow for tight AI write/verify loops.
  Progress in this pass:
  - done: increased non-cargo gate sharding for `prepush-standard` (`4 -> 6` on 8+ CPUs).
  - done: added adaptive cargo gate sharding defaults for heavy profiles.
  - done: added disk-aware health runner safeguards (auto aggressive reclaim on low disk, minimum free-space guard, automatic cargo shard downshift when headroom is low).
  Acceptance:
  prepush-standard runtime <= 300000ms on the same machine class, with no loss of required gate coverage.

## Execution Order (Recommended)

1. P0.1 (recover AI iteration speed first).
