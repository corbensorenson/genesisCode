# GenesisCode Red-Team Report (P0/P1 Active Risk Summary)

Last updated: 2026-02-25

Scope:
- Track unresolved `P0`/`P1` risks from `/Users/corbensorenson/Documents/genesisCode/upgrade_plan.md`.
- Keep active IDs synchronized with `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/selfhost_readiness_report.json`.

## Active Risks (P0/P1)

- `P0.1` - `gcpm lock/install` still local-first; missing remote-first dependency closure for autonomous builds.
- `P0.2` - host ABI has no first-class native FFI family (`host/ffi::*`) for high-throughput interop.
- `P0.3` - release lane lacks strict-sound type/effect gate; gradual-only checker weakens autonomous safety.
- `P0.4` - no deterministic intent-to-workflow planner contract in runtime/tooling core.
- `P1.1` - full profile throughput and disk-headroom behavior still increase autonomous CI latency.
- `P1.2` - stage2 optimizer coverage floors are not enforced by default artifact policy.
- `P1.3` - ecosystem bridge into GenesisPkg is not first-class for external dependency onboarding.
- `P1.4` - regulated assurance still depends on external-control integration outside toolchain closure.
