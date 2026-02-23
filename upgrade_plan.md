# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-23

This file contains only open items from the latest red-team pass.
Completed work must be removed from this file and kept in git history/release notes.
Machine-readable selfhost readiness source: `.genesis/perf/selfhost_readiness_report.json`.

Open checklist items: 4

## P0 - Self-Host and Agentic-Execution Blockers

- [ ] P0.1 Replace target-bundle contract scripts with real target runtime/deploy pipelines.
  Evidence:
  - `crates/gc_cli_driver/src/pkg_workspace_ops_build_artifacts.rs` emits `artifact/launch_*.sh` that return `boot-ok:*` / `smoke-ok:*` sentinels.
  - `scripts/check_gcpm_target_runtime_pipelines.sh` validates those sentinel strings instead of real runtime execution.
  Risk:
  - Agents can produce deterministic build artifacts but cannot produce truly runnable target outputs for iOS/Android/edge/service-runtime.
  Acceptance:
  - `gcpm build --target <...>` emits real executable/deployable artifacts per target contract.
  - Target smoke tests validate real entrypoint execution (not static `echo` sentinels).
  - `boot-ok/smoke-ok` sentinel checks are removed from production target pipeline gates.

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

## P1 - Agent-First Capability Gaps

- [ ] P1.4 Reduce heavy parity/performance lane runtime for release-grade agent feedback loops.
  Evidence:
  - `.genesis/perf/agent_workflow_runtime_parity_report.json` elapsed `168461ms`.
  - Full strict lanes remain expensive relative to rapid agent design/verify loops.
  Acceptance:
  - Reduce parity lane wall-time by at least 30% with no gate coverage loss.
  - Preserve deterministic history/p95 regression enforcement semantics.

## P2 - Program-Level Completion Gaps

- [ ] P2.2 Seed reproducible domain kit/package baselines for “build basically anything” bootstrap.
  Evidence:
  - Agent workflow and skill conformance are strong, but project-level bootstrap still depends heavily on in-repo templates and custom authoring.
  Acceptance:
  - Publish curated, signed starter bundles (service, game loop, GPU compute, data pipeline, plugin/FFI, XR) with lock snapshots and obligation evidence.
  - Ensure `gcpm add/install` from registry can bootstrap each domain without manual wiring.

## Execution Order (Recommended)

1. P0.1
2. P0.3
3. P1.4
4. P2.2
