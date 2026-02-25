# GenesisCode Red-Team Report (P0/P1 Active Risk Summary)

Last updated: 2026-02-25

Scope:
- Track unresolved `P0`/`P1` risks from `/Users/corbensorenson/Documents/genesisCode/upgrade_plan.md`.
- Keep active IDs synchronized with `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/selfhost_readiness_report.json`.

## Active Risks (P0/P1)

- P0.1 - Expand stage2 translation-validation coverage so selfhost/agent modules stop hitting `Stage2CompileError::Unsupported` for valid CoreForm programs.
- P0.2 - Complete first-party backend bridge semantics for `io/net::*` + `sys/process::*` lifecycle ops (listen/accept/send/recv/close and real spawn/wait/kill behavior).
- P1.1 - Replace deterministic target wrapper artifacts with real deployment packagers for `ios`, `android`, `edge`, and `service-runtime` targets.
- P1.2 - Remove remaining manual backend bootstrap debt outside workspace-scaffolded flows, including WASI remote registry/sync paths.
- P1.3 - Expand first-party plugin/ffi bridge coverage from demo/limited ABI helpers to schema-driven general host ABI execution.
