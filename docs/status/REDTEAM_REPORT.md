# GenesisCode Red-Team Report

Last updated: 2026-02-21

Scope:
- Track unresolved `P0` and `P1` risks from `/Users/corbensorenson/Documents/genesisCode/upgrade_plan.md`.
- Keep entries actionable for self-hosted, AI-first v1 cutover.

## Active Risks (P0/P1)

- `P1.1` Browser runtime host profile + ABI family missing.
  - Risk: wasm-host agent workloads cannot target first-class browser runtime surfaces without custom bridge glue.
- `P1.2` WebXR capability family missing.
  - Risk: no first-class XR session/frame/input/haptics contracts for VR/AR products.
- `P1.3` Inbound/server networking primitives missing.
  - Risk: Genesis-native service hosting is limited without deterministic listen/accept/http-serve/ws-accept contracts.
- `P1.4` Durable data capability family missing.
  - Risk: complex applications require external ad-hoc storage wiring instead of policy-gated deterministic data contracts.
- `P1.5` Deterministic deployment/bundle targets missing in `gcpm`.
  - Risk: AI-generated projects lack a canonical shipping pipeline for web/desktop/service targets.
- `P1.6` Agent gauntlet does not yet cover browser/xr/server/data/deploy domains.
  - Risk: release readiness can pass while critical product surfaces remain unvalidated.
