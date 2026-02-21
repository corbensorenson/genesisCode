# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-21

This file contains only unresolved findings from the latest red-team pass.
Completed items are intentionally removed.

Open checklist items: 7

## P0 - Selfhost Cutover Blockers

- [ ] P0.1 Enforce artifact-only bootstrap for production WASM selfhost APIs.
  Why this matters:
  - Native/WASI CLI paths are strict, but `gc_wasm` still exposes production-facing selfhost calls that bootstrap without an explicit artifact (`None` path), weakening artifact authority for browser/Node host integrations.
  Evidence:
  - `crates/gc_wasm/src/lib.rs:66`
  - `crates/gc_wasm/src/lib.rs:136`
  - `crates/gc_wasm/src/lib.rs:298`
  - `crates/gc_wasm/src/lib.rs:453`
  - `crates/gc_wasm/src/lib.rs:496`
  Exit criteria:
  - WASM production API defaults require explicit artifact input (or pinned artifact identity) for selfhost frontend paths.
  - Non-artifact bootstrap remains parity-harness/dev-only.
  - Add regression tests that fail when wasm32 production paths silently fallback to embedded/bootstrap sources.

## P1 - AI-First Capability and Throughput Gaps

- [ ] P1.1 Expand network capability surface beyond `io/net::http-request`.
  Why this matters:
  - Agent-built systems (multiplayer, realtime collaboration, distributed workers) need first-class socket/stream primitives; HTTP-only capability narrows the buildable application envelope.
  Evidence:
  - `docs/spec/HOST_ABI.md:25`
  - `docs/spec/HOST_ABI.md:151`
  Exit criteria:
  - Add policy-gated deterministic network ops for at least one stream transport family (for example TCP and/or WebSocket).
  - Document payload/result schema, replay behavior, and deny-by-default policy controls.
  - Add host ABI conformance and replay tests for new ops.

- [ ] P1.2 Add a generic host extension ABI (not editor-only plugin dispatch).
  Why this matters:
  - Current bridge-mediated plugin operation is editor-scoped (`editor/plugin::command`), which limits non-editor extension scenarios for AI-authored systems.
  Evidence:
  - `docs/spec/HOST_ABI.md:29`
  - `docs/spec/HOST_ABI.md:97`
  Exit criteria:
  - Introduce a neutral extension op family (for example `host/plugin::*`), with strict policy and deterministic replay contract.
  - Keep `editor/plugin::*` as a domain wrapper over the generic host extension surface.

- [ ] P1.3 Promote runtime backend profile selection into `gcpm` workflows.
  Why this matters:
  - Runtime capability posture is currently compile-time Cargo feature selection, with default `profile-headless`; agent workflows need a first-class project/runtime profile contract, not manual feature toggles.
  Evidence:
  - `docs/spec/RUNTIME_BACKEND_PROFILES_v0.1.md:12`
  - `docs/spec/RUNTIME_BACKEND_PROFILES_v0.1.md:25`
  Exit criteria:
  - `gcpm` exposes deterministic runtime profile selection for workspace/project execution.
  - Profile selection is reflected in machine-readable outputs (`--json`) and env realization artifacts.
  - CI validates profile-to-capability mapping end-to-end.

- [ ] P1.4 Require device-backed GPU compute conformance in release lanes.
  Why this matters:
  - Current perf/SLO reports can pass with `deterministic-fallback`; this is useful for dev determinism, but insufficient as the sole release signal for production GPU compute readiness.
  Evidence:
  - `.genesis/perf/concurrency_gpu_slo_report.json` (`"gpu_compute_backend":"deterministic-fallback"`)
  - `docs/spec/CONCURRENCY_GPU_SLO_v0.1.md:29`
  Exit criteria:
  - Release/full CI profiles include a required `device-runtime` conformance lane.
  - Fallback remains available for dev/test profiles only and is explicitly marked non-release.

- [ ] P1.5 Enforce measured wall-time budgets for strict/full test profiles.
  Why this matters:
  - The matrix guard currently validates documentation/step presence but does not fail on observed runtime regressions for `strict-golden` and `full-cross-host`.
  Evidence:
  - `scripts/check_test_execution_profile_matrix.sh:1`
  - `docs/spec/TEST_EXECUTION_PROFILES_v0.1.md:14`
  Exit criteria:
  - Add runtime-budget enforcement using emitted profile reports (not docs-only checks) for strict/full lanes.
  - Track and gate on historical p95 regressions for these profiles.

- [ ] P1.6 Continue decomposition of high-churn Rust and `.gc` hotspots for AI edit reliability.
  Why this matters:
  - Remaining 500-1000 line hotspots still increase agent patch blast radius and regression risk in core surfaces.
  Evidence:
  - `selfhost/patch_schema_v1.gc` (692 lines)
  - `prelude/modules/10_gfx_ui_runtime.gc` (615 lines)
  - `crates/gc_effects/src/runner_task.rs` (969 lines)
  - `crates/gc_cli_driver/src/lib.rs` (966 lines)
  Exit criteria:
  - Split highest-change hotspot files into narrower modules with ownership boundaries.
  - Preserve behavior via existing strict/golden suites and source-size guard updates.
