# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-22

This file contains only unresolved findings from the latest red-team pass.
Completed items are intentionally removed.

Open checklist items: 1

- [ ] P3.1 Break up oversized `gc_effects` dispatch sources below source-size policy limits: split `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_capability_dispatch.rs` (2892 lines) and `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_capability_dispatch_tests.rs` (2173 lines) into maintainable modules so `scripts/check_source_size_budget.sh` passes without policy carve-outs.
