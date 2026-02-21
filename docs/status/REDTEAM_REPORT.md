# GenesisCode Red-Team Report

Last updated: 2026-02-21

Scope:
- Track unresolved `P0` and `P1` risks from `/Users/corbensorenson/Documents/genesisCode/upgrade_plan.md`.
- Keep entries actionable for self-hosted, AI-first v1 cutover.

## Active Risks (P0/P1)

- `P0.1` Production WASM selfhost APIs still permit non-artifact bootstrap defaults.
  - Evidence: `crates/gc_wasm/src/lib.rs` selfhost entrypoints pass `None` artifact path for default flows.
  - Next action: enforce artifact-only bootstrap defaults for production wasm paths and add regression guards.

- `P1.1` Network capability surface remains HTTP-only (`io/net::http-request`), limiting agent-built realtime/distributed workflows.
  - Evidence: `docs/spec/HOST_ABI.md` stable op list.
  - Next action: design and ship deterministic stream/socket capability ops with policy and replay contracts.

- `P1.2` Generic host extension ABI is incomplete; current plugin bridge is editor-scoped.
  - Evidence: `editor/plugin::command` is the only bridge-mediated plugin surface in host ABI docs.
  - Next action: add neutral host extension op family and keep editor plugin calls layered on top.

- `P1.3` Runtime backend profile selection is compile-time only and not fully surfaced through `gcpm`.
  - Evidence: `docs/spec/RUNTIME_BACKEND_PROFILES_v0.1.md` (Cargo feature profiles, default headless).
  - Next action: promote runtime profile selection into workspace/project manager contracts.

- `P1.4` GPU compute release readiness is not fail-closed on device runtime.
  - Evidence: perf SLO report can pass with `gpu_compute_backend = deterministic-fallback`.
  - Next action: require device-runtime conformance lane in release/full CI.

- `P1.5` Strict/full profile runtime budgets are documented but not enforced by measured wall-time regression gates.
  - Evidence: matrix guard validates docs/step presence, not observed elapsed metrics for strict/full lanes.
  - Next action: add measured profile budget gating (p95 regressions) for strict/full suites.

- `P1.6` Remaining large Rust/GC hotspots still increase AI patch blast radius in core paths.
  - Evidence: top hotspots remain in `selfhost/patch_schema_v1.gc`, `prelude/modules/10_gfx_ui_runtime.gc`, `crates/gc_effects/src/runner_task.rs`, `crates/gc_cli_driver/src/lib.rs`.
  - Next action: continue decomposition into narrower ownership modules while holding strict suites green.
