# GenesisCode Red-Team Report (P0/P1 Active Risk Summary)

Last updated: 2026-02-25

Scope:
- Track unresolved `P0`/`P1` risks from `/Users/corbensorenson/Documents/genesisCode/upgrade_plan.md`.
- Keep active IDs synchronized with `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/selfhost_readiness_report.json`.

## Active Risks (P0/P1)

- `P0.1` Full selfhost closure still depends on explicit Rust semantic/runtime exceptions (`gc_coreform`, `gc_kernel`, `gc_prelude`, `gc_effects`, `gc_cli_driver`).
- `P0.2` Selfhost readiness critical-gate truth does not yet include generative workload and cross-runtime parity fail-closed checks.
- `P1.1` GCPM operation contract pack currently tracks only a narrow subset of operations, creating drift risk for autonomous command planning.
- `P1.2` Agent gauntlet still allows fallback-backed GPU success in default confidence lanes (`require_gpu_device_backend=false`).
- `P1.3` Hot-path runtime profile quality is under-constrained (very high wall budget with low recent history depth), reducing regression sensitivity.
