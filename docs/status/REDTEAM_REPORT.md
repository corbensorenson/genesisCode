# GenesisCode Red-Team Report

Last updated: 2026-02-21

Scope:
- Track unresolved `P0` and `P1` risks from `/Users/corbensorenson/Documents/genesisCode/upgrade_plan.md`.
- Keep entries actionable for self-hosted, AI-first v1 cutover.

## Active Risks (P0/P1)

- `P1.6` Remaining large Rust/GC hotspots still increase AI patch blast radius in core paths.
  - Evidence: top hotspots remain in `selfhost/patch_schema_v1.gc`, `prelude/modules/10_gfx_ui_runtime.gc`, `crates/gc_effects/src/runner_task.rs`, `crates/gc_cli_driver/src/lib.rs`.
  - Next action: continue decomposition into narrower ownership modules while holding strict suites green.

- `P1.8` Structural coverage obligations do not yet support decision/MC/DC assurance profiles.
  - Evidence: current coverage contract is export symbol hit coverage only.
  - Next action: add deterministic structural coverage collection and profile-mapped gating.
