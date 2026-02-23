# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-23

This file contains only open items from the latest red-team pass.
Completed work must be removed from this file and kept in git history/release notes.
Machine-readable selfhost readiness source: `.genesis/perf/selfhost_readiness_report.json`.

Open checklist items: 2

## P0 - Self-Host and Agentic-Execution Blockers

- [ ] P0.3 Finish `in-progress` Rust->GC migrations to GC-first production dispatch (`phase-2`).
  Evidence: `docs/spec/GC_MODULE_BOUNDARIES_v0.1.md` rows still `in-progress`:
  - `crates/gc_cli_driver/src/cmd_selfhost.rs`
  - `crates/gc_cli_driver/src/pkg_workspace_ops.rs`
  - `crates/gc_gfx/src/lib.rs`
  - `crates/gc_prelude/src/prelude.rs`
  - `crates/gc_effects/src/runner_host_bridge.rs`
  Acceptance:
  - Production path routes through GC-owned modules first.
  - Rust path retained parity-only or archived under `old_bootstrap`.
  - Existing parity gates remain green after cutover.

## P2 - Program-Level Completion Gaps

- [ ] P2.2 Seed reproducible domain kit/package baselines for “build basically anything” bootstrap.
  Evidence:
  - Agent workflow and skill conformance are strong, but project-level bootstrap still depends heavily on in-repo templates and custom authoring.
  Acceptance:
  - Publish curated, signed starter bundles (service, game loop, GPU compute, data pipeline, plugin/FFI, XR) with lock snapshots and obligation evidence.
  - Ensure `gcpm add/install` from registry can bootstrap each domain without manual wiring.

## Execution Order (Recommended)

1. P0.3
2. P2.2
