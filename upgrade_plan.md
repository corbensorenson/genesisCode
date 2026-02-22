# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-22

This file contains only unresolved findings from the latest red-team pass.
Completed items are intentionally removed.

Open checklist items: 0

## P1 - Productization blockers for the "agent can build anything" target

- [x] P1.1 Add first-class XR haptics capability lane.
  - Evidence: no `haptic*` operations exist in `/Users/corbensorenson/Documents/genesisCode/docs/spec/XR_HOST_RUNTIME_v0.1.md`, `/Users/corbensorenson/Documents/genesisCode/docs/spec/HOST_ABI.md`, or capability index artifacts.
  - Exit criteria: canonical `gfx/xr::haptics-*` ABI/schema/index entries, prelude wrappers, runtime dispatch, policy gates, replay parity tests, and gauntlet coverage.
  - Completed 2026-02-22: added canonical `gfx/xr::haptics-pulse` dispatch + policy gates in `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_xr_host.rs`, prelude/domain-kit wrappers in `/Users/corbensorenson/Documents/genesisCode/prelude/modules/10_xr_host.gc` and `/Users/corbensorenson/Documents/genesisCode/prelude/modules/34_xr_workflow.gc`, workflow gauntlet updates under `/Users/corbensorenson/Documents/genesisCode/examples/agent_xr_runtime_workflow/`, schema/index regeneration via `/Users/corbensorenson/Documents/genesisCode/scripts/generate_capability_indices.py`, and replay parity coverage in `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/tests/host_abi_surface.rs` + `/Users/corbensorenson/Documents/genesisCode/crates/gc_prelude/tests/prelude_xr_wrappers.rs`.

- [x] P1.2 Add real WebXR device backend path (not simulator-only) with deterministic replay envelopes.
  - Evidence: current XR runtime contract is explicitly simulator-backed (`:adapter = "xr-headless-sim"` in `/Users/corbensorenson/Documents/genesisCode/docs/spec/XR_HOST_RUNTIME_v0.1.md`).
  - Exit criteria: browser/device-backed XR backend lane with deterministic event capture/replay, policy profiles, and native-vs-wasi parity checks.
  - Completed 2026-02-22: added explicit `xr_backend = "webxr-device"` lane in `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_xr_host.rs` with fail-closed bridge requirements and deterministic per-op `:replay-envelope` capture metadata, added canonical policy template `/Users/corbensorenson/Documents/genesisCode/docs/policies/xr_webxr_device_caps_v0.1.toml`, updated XR runtime/ABI docs in `/Users/corbensorenson/Documents/genesisCode/docs/spec/XR_HOST_RUNTIME_v0.1.md` and `/Users/corbensorenson/Documents/genesisCode/docs/spec/HOST_ABI.md`, and validated replay + cross-runtime parity via `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/tests/host_abi_surface.rs` (`xr_webxr_device_backend_ops_are_replay_deterministic_with_wasi_bridge_profile`) plus native-vs-wasi workflow parity check for `/Users/corbensorenson/Documents/genesisCode/examples/agent_xr_runtime_workflow/workflow.sh`.

- [x] P1.3 Complete compute/graphics surface decoupling and retire graphics-namespaced compute compatibility aliases.
  - Evidence: compatibility alias path still active (`legacy gfx/gpu compute aliases` in `/Users/corbensorenson/Documents/genesisCode/docs/spec/HOST_ABI.md` and alias normalization in `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_response_budget.rs`).
  - Exit criteria: `gpu/compute::*` is the only compute canonical surface in production paths; compatibility wrappers removed or hard-gated to parity harness only.
  - Completed 2026-02-22: removed compute alias normalization from `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_response_budget.rs`, retired `gfx/gpu` compute alias handling in `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_gpu_host.rs`, `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_gpu_backend_policy.rs`, and `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_gpu_device_backend.rs`, removed compatibility wrapper symbols from `/Users/corbensorenson/Documents/genesisCode/prelude/modules/10_gfx.gc`, updated policy/docs in `/Users/corbensorenson/Documents/genesisCode/docs/policies/gpu_device_runtime_caps_v0.1.toml`, `/Users/corbensorenson/Documents/genesisCode/docs/spec/HOST_ABI.md`, and `/Users/corbensorenson/Documents/genesisCode/docs/spec/GFX_CAPS.md`, and validated with host ABI/prelude/gpu backend conformance tests.

- [x] P1.4 Add standards-oriented assurance profile packs for regulated delivery programs.
  - Evidence: no dedicated DO-178C/NASA NPR 7150.2/IEC 62304 profile or crosswalk artifacts currently exist in `/Users/corbensorenson/Documents/genesisCode/docs/spec`.
  - Exit criteria: explicit policy/profile templates, evidence export contracts, and reproducible assurance-pack mapping docs for those standards families.
  - Completed 2026-02-22: added canonical profile templates in `/Users/corbensorenson/Documents/genesisCode/policies/assurance/profile_packs.toml`, standards crosswalk + deterministic export contract documentation in `/Users/corbensorenson/Documents/genesisCode/docs/spec/ASSURANCE_PROFILE_PACKS_v0.1.md`, bundle/CLI wiring in `/Users/corbensorenson/Documents/genesisCode/docs/spec/GCPM_BUNDLE_v0.1.md` and `/Users/corbensorenson/Documents/genesisCode/docs/spec/CLI.md`, and fail-closed drift guard coverage via `/Users/corbensorenson/Documents/genesisCode/scripts/check_assurance_profile_packs.sh` wired into CI and upgrade-plan health gates.

## P2 - Hardening, optimization, and AI-authoring scale

- [x] P2.1 Make red-team status guards bidirectional so stale active-risk entries fail CI.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/scripts/check_redteam_report.sh` only checks for missing unresolved P0/P1 IDs, not extra stale IDs in `/Users/corbensorenson/Documents/genesisCode/docs/status/REDTEAM_REPORT.md`.
  - Exit criteria: guard fails on missing-or-extra risk IDs and enforces a canonical "no active risks" state when backlog has no P0/P1 items.
  - Completed 2026-02-22: guard now fails on stale extra IDs, supports canonical no-risk state validation, and includes fixture-path overrides for deterministic self-tests.

- [x] P2.2 Enforce scenario/cross-host perf regression gates with statistically meaningful history depth.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/agent_scenario_perf_report.json` shows `p95_enforced=false` (sample_count=1); `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/full_cross_host_profile_report.json` has only 2 history samples.
  - Exit criteria: seeded baseline history + CI minimum-sample enforcement so p95/regression budgets are always active in release-facing lanes.
  - Completed 2026-02-22: added baseline seed histories under `/Users/corbensorenson/Documents/genesisCode/policies/perf/` plus fail-closed minimum-history enforcement in both scenario and full-cross-host runtime budget gates.

- [x] P2.3 Reduce lock-path variance in AI iteration loop to keep perf gates stable.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/ai_iteration_slo_metrics.json` reports lock-path spread warning (`gcpm_lock_ms sample spread 76.94%`).
  - Exit criteria: lock/env loop variance below contention warning threshold across baseline runs without relaxing budgets.
  - Completed 2026-02-22: added deterministic warm-up + stabilization retry windows for `gcpm lock/env` measurements in `/Users/corbensorenson/Documents/genesisCode/scripts/check_ai_iteration_slo.sh`; current report shows no contention warnings and lock/env spread ~2.5%.

- [x] P2.4 Continue splitting large production modules to improve maintainability and agent edit locality.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/scripts/check_source_size_budget.sh` reports large files (`runner_capability_dispatch.rs` and related hot files still among top line counts).
  - Exit criteria: capability dispatch and adjacent runtime modules are further decomposed into focused units with unchanged behavior and passing conformance/perf gates.
  - Completed 2026-02-22: extracted `io/net::*` policy and handler surface from `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_capability_dispatch.rs` into `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_capability_dispatch/net.rs`, reducing primary dispatcher size (1358 -> 631 LOC) while keeping behavior stable (`cargo test -p gc_effects runner_capability_dispatch`, source-size gate, and doc-hygiene gate all pass).

- [x] P2.5 Consolidate documentation further around bundle entrypoints and reduce long-tail doc surface.
  - Evidence: repository currently has 100+ markdown docs and deprecated redirect stubs remain (`doc-hygiene: deprecated_docs=6`).
  - Exit criteria: more split docs folded into canonical bundles, duplicate guidance removed, index/deprecation map updated, and doc-hygiene remains green.
  - Completed 2026-02-22: consolidated graphics demo guidance into `/Users/corbensorenson/Documents/genesisCode/docs/spec/GPU_GFX_BUNDLE_v0.1.md` (`Demo Workloads`), converted `/Users/corbensorenson/Documents/genesisCode/docs/GFX_DEMOS.md` into a strict redirect stub, and updated `/Users/corbensorenson/Documents/genesisCode/docs/DEPRECATION_MAP_v0.1.md`, `/Users/corbensorenson/Documents/genesisCode/docs/INDEX.md`, `/Users/corbensorenson/Documents/genesisCode/docs/GETTING_STARTED.md`, and `/Users/corbensorenson/Documents/genesisCode/docs/spec/GFX_ARCH.md`; doc-hygiene remains green.

- [x] P2.6 Expand from fixed reference workflows to generative agent workload validation.
  - Evidence: `/Users/corbensorenson/Documents/genesisCode/scripts/check_agent_reference_workflows.sh` validates a fixed list of 17 scripted workflows.
  - Exit criteria: property-based/generative workload suite that mutates workflow shapes/contracts and feeds parity/perf gates beyond the static scenario list.
  - Completed 2026-02-22: added `/Users/corbensorenson/Documents/genesisCode/scripts/check_agent_generative_workloads.sh` with deterministic mutation-based workload generation, duration/domain/replay invariants, parity-mode comparisons, CI gate wiring, and release/parity health-profile integration.
