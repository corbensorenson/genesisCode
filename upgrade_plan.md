# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-23

This file contains only open items from the latest red-team pass.
Completed work must be removed from this file and kept in git history/release notes.
Machine-readable selfhost readiness source: `.genesis/perf/selfhost_readiness_report.json`.

Open checklist items: 2

## P0 - Self-Host and Iteration Blockers

- [ ] P0.1 Cut strict prepush lane wall-time to AI-usable latency.
  Evidence:
  `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/upgrade_plan_health_profile_report.json` shows:
  `elapsed_ms = 675910`, `profile = "prepush-standard"`, `gate_count = 67`.
  This is ~11.3 minutes, too slow for tight AI write/verify loops.
  Acceptance:
  prepush-standard runtime <= 300000ms on the same machine class, with no loss of required gate coverage.

## P1 - AI-First Maintainability and Runtime Hardening

- [ ] P1.1 Decompose high-churn Rust modules near the line cap to improve AI edit locality.
  Evidence:
  `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/source_decomposition_progress_report.json` and source scan show core files near 1k lines:
  - `crates/gc_cli_driver/src/semantic_workspace.rs` (983)
  - `crates/gc_obligations/src/obligation_exec.rs` (977)
  - `crates/gc_gfx/src/lib.rs` (972)
  - `crates/gc_patches/src/lib.rs` (943)
  Acceptance:
  each of the top-10 production hotspots reduced to <= 700 lines with stable module boundaries and unchanged behavior.

- [x] P1.2 Decompose largest selfhost/prelude `.gc` modules for AI readability.
  Evidence:
  size scan shows large authoring modules:
  - `prelude/modules/10_gfx.gc` (574)
  - `prelude/modules/20_editor_lint.gc` (531)
  - `selfhost/printer.gc` (510)
  - `prelude/modules/10_gfx_runtime_trace.gc` (504)
  Acceptance:
  split these modules into focused submodules with deterministic import order and unchanged canonical output.
  Status:
  completed on 2026-02-23 by splitting into ordered submodules:
  - `prelude/modules/10_gfx_{00_gpu_scene,01_frame_desc,02_2d_host}.gc`
  - `prelude/modules/20_editor_lint_{00_core,01_module,02_panel_obligation}.gc`
  - `prelude/modules/10_gfx_runtime_trace_{00_plan_trace,01_reports,02_budget_api}.gc`
  - `selfhost/printer/{00_core_single_line,01_single_line_list,02_fmt_structured,03_fmt_list_module}.gc`
  and regenerating `prelude/prelude.gc` + `selfhost/toolchain.gc` with passing equivalence checks.

## Execution Order (Recommended)

1. P0.1 (recover AI iteration speed first).
2. P1.1 + P1.2 (structural decomposition for maintainability).
