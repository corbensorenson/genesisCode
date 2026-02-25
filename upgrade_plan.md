# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-25

Scope:
- Track only unresolved upgrades required for AI-first authoring reliability, selfhost closure, and productization trust.
- Keep this file machine-syncable with `.genesis/perf/selfhost_readiness_report.json` and `feature_matrix.md`.
- Keep completed work out of this file (git history + perf artifacts are closure evidence).

Open checklist items: 2

## Critical Path

- P0.1 - turnkey host backend provisioning for agent execution
- P0.2 - stage2 compiler coverage for arbitrary agent-generated programs

## Unresolved Backlog

- [ ] P0.1 Turnkey host backend provisioning for agent execution
Why: key capability families still fail closed with `core/caps/backend-unavailable` unless explicit per-op bridge policy is hand-authored, which blocks autonomous agent execution outside curated demos.
Done when:
  - `gcpm env --profile backend` can materialize signed, policy-pinned bridge/runtime bundles for `io/net::*`, `io/db::*`, `sys/process::*`, `core/crypto::*`, `host/plugin::*`, `host/ffi::*`, `editor/*`, and gfx/gpu families without manual caps edits.
  - generated capability policies include deterministic allowlists/digest pins and pass replay invariants on first boot.
  - default starter workflows can run end-to-end in a clean workspace with zero manual bridge configuration.
Evidence:
  - `crates/gc_effects/src/runner_capability_dispatch.rs`
  - `docs/spec/CAPS_TOML.md`
  - `docs/spec/HOST_BRIDGE_PROTOCOL.md`

- [ ] P0.2 Stage2 compiler coverage for arbitrary agent-generated programs
Why: stage2 still rejects valid high-level program patterns (for example recursive expansion paths), which prevents using optimized/gated compilation as a universal execution lane for unconstrained agent output.
Done when:
  - stage2 supports recursive/tail-recursive and higher-order patterns required by generated workloads, or provides deterministic validated lowering for equivalent forms.
  - stage2 gate runs fail-closed over the generative workload corpus without unsupported-form failures.
  - `selfhost/toolchain.gc` stage2-supported/validated module gates increase beyond the current floor and are enforced in release profiles.
Evidence:
  - `crates/gc_opt/src/stage2_wasm/expr_lowering.rs`
  - `docs/spec/WASM.md`
  - `.genesis/perf/full_selfhost_cutover_profile_report.json`

## Evidence Anchors

- `.genesis/perf/selfhost_readiness_report.json`
- `.genesis/perf/full_selfhost_cutover_profile_report.json`
- `.genesis/perf/agent_capability_gauntlet_release_confidence_report.json`
- `.genesis/perf/agent_generative_workloads_report.json`
- `.genesis/perf/agent_workflow_runtime_parity_report.json`
- `.genesis/perf/gcpm_operation_contract_pack_report.json`
- `.genesis/perf/hot_path_runtime_report.json`
- `.genesis/perf/upgrade_plan_health_profile_report.json`
