# GenesisCode Upgrade Plan - Red-Team Backlog (Unresolved Only)

Last updated: 2026-02-25

Scope:
- Track only unresolved upgrades required for AI-first authoring reliability, selfhost closure, and productization trust.
- Keep this file machine-syncable with `.genesis/perf/selfhost_readiness_report.json` and `feature_matrix.md`.
- Keep completed work out of this file (git history + perf artifacts are closure evidence).

Open checklist items: 0

## Critical Path

- [x] P0.1 Expand stage2 translation-validation coverage so selfhost/agent modules stop hitting `Stage2CompileError::Unsupported` for valid CoreForm programs.
  - [x] Stage2 `eval_original_data` now handles `Value::EffectProgram` via deterministic projection (`:stage2/value-kind "effect-program"` + stable effect-program hash) instead of hard unsupported failure.
  - [x] Translation-validation for effectful top-level modules now passes through constant fallback paths with replay-stable term/value-hash comparisons.
  - [x] Added stage2 regression test coverage for effect-program projection and validation (`stage2_validates_effect_program_via_deterministic_projection`).
- [x] P0.2 Complete first-party backend bridge semantics for `io/net::*` + `sys/process::*` lifecycle ops (listen/accept/send/recv/close and real spawn/wait/kill behavior).
  - [x] `sys/process::*` bridge path now runs real async process lifecycle with persisted runtime state (`spawn`/`wait`/`kill`/`stdin-write`/`stdout-read`/`stderr-read`) instead of synchronous snapshot simulation.
  - [x] `io/net::tcp-*` + `io/net::udp-*` first-party bridge handlers now implement runtime lifecycle semantics (`listen`/`accept`/`open`/`send`/`recv`/`close`) instead of hard `unsupported` errors.
  - [x] `io/net::http-listen|http-respond|ws-accept|ws-open|ws-send|ws-recv|ws-close` now execute through first-party runtime semantics with listener/request and ws stream lifecycle state.
- [x] P0.3 Replace non-production first-party crypto bridge primitives with production-grade algorithms/key-provider integration while preserving deterministic logs/replay contracts.

## Unresolved Backlog

- [x] P1.1 Replace deterministic target wrapper artifacts with real deployment packagers for `ios`, `android`, `edge`, and `service-runtime` targets.
  - [x] `ios` and `android` build targets now emit deterministic platform-style zip bundles (`.ipa`/`.aab`) with target runtime descriptors and packaged entrypoints, replacing CoreForm wrapper payloads.
  - [x] `edge` and `service-runtime` build targets now emit real wasm modules (magic/version + exported entry) with deterministic metadata custom sections, replacing wrapper map payloads.
  - [x] Build/test contracts now assert payload-kind semantics (`ios-ipa-zip-v1`, `android-aab-zip-v1`, `edge-wasm-module-v1`, `service-runtime-wasm-module-v1`) and validate artifact bytes accordingly.
- [x] P1.2 Remove remaining manual backend bootstrap debt outside workspace-scaffolded flows, including WASI remote registry/sync paths.
  - [x] Backend profile env materialization no longer copies/mirrors runtime binaries; it now resolves an existing bridge command or provisions an in-workspace launcher shim only.
  - [x] Backend bridge command policy remains path-contained within workspace runtime roots, preserving capability sandbox guarantees without manual bridge setup.
  - [x] WASI remote bridge autodiscovery now requires a generated runtime descriptor (`runtime.gc`) under `.genesis/runtime/wasi-http-bridge`, eliminating ad-hoc directory-only discovery paths.
- [x] P1.4 Restore `agent-capability-gauntlet` release-confidence lane to `ok=true` by closing workflow/domain coverage failures.
  - [x] Release-confidence gauntlet now completes with `workflow_successes=25/25`, `domain_successes=23/23`, `score_percent=100.0`, and `ok=true`.
  - [x] Durable data workflow regression root cause fixed (`gc_cli_driver_parity` missing `sha1` dependency in release-confidence compile path).
- [x] P1.3 Expand first-party plugin/ffi bridge coverage from demo/limited ABI helpers to schema-driven general host ABI execution.
  - [x] `host/plugin::command` first-party runtime now supports schema-driven execution paths (`genesis/plugin.request.exec.v1`, `genesis/plugin.request.jsonrpc.v1`) and typed bytes/result responses instead of demo-only semantics.
  - [x] `host/ffi::call` first-party runtime now supports schema-driven general ABI execution via external command-backed libraries (including structured `:ok false/:error` envelopes), replacing hard `unsupported ffi call` fallthrough for non-builtin ABI symbols.
  - [x] Added first-party runtime tests covering typed plugin exec-bytes flow, schema-driven external FFI execution, and structured FFI spawn-failure envelopes.
- [x] P2.1 Add a large-workspace agent-performance lane (>=10k module corpus) with enforced SLOs for `gcpm lock/build/test` and selfhost artifact refresh.

## Evidence Anchors

- `.genesis/perf/selfhost_readiness_report.json`
- `.genesis/perf/full_selfhost_cutover_profile_report.json`
- `.genesis/perf/agent_capability_gauntlet_release_confidence_report.json`
- `.genesis/perf/agent_generative_workloads_report.json`
- `.genesis/perf/agent_workflow_runtime_parity_report.json`
- `.genesis/perf/backend_starter_workflows_report.json`
- `.genesis/perf/domain_starter_registry_bootstrap_report.json`
- `.genesis/perf/gcpm_operation_contract_pack_report.json`
- `.genesis/perf/large_workspace_agent_perf_report.json`
- `.genesis/perf/large_workspace_agent_runtime_report.json`
- `.genesis/perf/hot_path_runtime_report.json`
- `.genesis/perf/upgrade_plan_health_profile_report.json`
- `crates/gc_opt/src/stage2_wasm.rs`
- `crates/gc_opt/src/stage2_wasm/pipeline_exec.rs`
- `crates/gc_cli_driver/src/host_bridge_runtime.rs`
- `crates/gc_cli_driver/src/host_bridge_runtime_host_abi.rs`
- `crates/gc_cli_driver/src/host_bridge_runtime_tests.rs`
- `crates/gc_cli_driver/src/pkg_workspace_ops_build_artifacts.rs`
- `docs/spec/HOST_ABI.md`
