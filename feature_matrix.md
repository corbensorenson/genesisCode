# GenesisCode Feature Matrix (Audit Date: 2026-02-26)

Last updated: 2026-02-26  
Scope: capabilities that matter for AI-agent autonomy, selfhost closure, and production runtime trust.

Legend:
- `✅` production-capable and validated in active gates
- `⚠️` available but still materially constrained for "agent can build anything" usage
- `❌` not first-class

## Core Competitive Matrix

| Capability | GenesisCode | Rust | Go | TypeScript (Node) | Python | Zig |
|---|---|---|---|---|---|---|
| Pure deterministic kernel separated from effects | ✅ | ⚠️ | ❌ | ❌ | ❌ | ❌ |
| Canonical CoreForm IR + stable content hash identity | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Sealed unforgeable `UNHANDLED`/`EFFECT`/`ERROR` protocol | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Deny-by-default capability policy runtime | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Deterministic effect logs + replay checker | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Built-in semantic VCS (`commit`/`patch`/`refs`/`merge3`) | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Built-in package/project manager (`pkg`/`gcpm`) | ✅ | ✅ | ✅ | ⚠️ | ⚠️ | ✅ |
| Reachability-based artifact GC (`refs` + locks + pins) | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Obligation/evidence/attestation-gated publish + ref updates | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Native + WASI + wasm-host runtime surfaces | ✅ | ⚠️ | ⚠️ | ✅ | ✅ | ⚠️ |
| Selfhost frontend default in production CLIs | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Full selfhost cutover profile + readiness scorecard | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Strict no-production Rust semantic fallback guard | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| CLI + GCPM JSON schema contracts for agent automation | ✅ | ⚠️ | ⚠️ | ✅ | ⚠️ | ❌ |
| Agent index + skill-pack conformance contracts | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Domain starter registry for agent workflows | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Agent generative workload parity gates (native vs WASI) | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Large-workspace agent iteration perf lane | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Concurrency/task replay stress lane | ✅ | ⚠️ | ✅ | ⚠️ | ⚠️ | ✅ |
| GPU compute (device-backed, deterministic lane) | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Graphics/window/input/audio runtime families | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Browser + XR runtime families | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Host plugin + FFI schemas/contracts | ⚠️ | ✅ | ⚠️ | ✅ | ✅ | ✅ |
| First-party bridge for network/process/db/crypto/plugin/ffi | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Stage2 CoreForm->WASM translation-validation | ✅ | ✅ | ✅ | ⚠️ | ⚠️ | ✅ |
| Deployment target pipeline in core toolchain | ✅ | ✅ | ✅ | ✅ | ⚠️ | ✅ |
| Assurance profile packs + standards crosswalk | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Tool qualification lineage + evidence closures | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| AI-first modular decomposition + boundary guards | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |

## Capability Family Coverage (from audit)

Source: `docs/spec/CAPABILITY_COVERAGE_AUDIT_v0.1.json`

- Family count: `33`
- Implemented families: `18`
- Planned families: `14`
- Policy-disabled families: `1` (`host/ffi`, deny-by-default)
- Host operations tracked: `187`
- Prelude operations tracked: `184`
- Planned upgrade IDs still open in capability coverage: `P0.2`, `P0.3`, `P0.4`

Planned families currently preventing "agent can build anything" confidence:
- `core/store`, `core/sync`, `core/pkg-low` (remote-registry parity)
- `gpu/compute`, `gfx/gpu` (device-runtime parity and placeholder retirement)
- `browser/window`, `browser/input`, `browser/audio`, `browser/storage`
- `gfx/window`, `gfx/input`, `gfx/audio`, `gfx/time`, `gfx/xr`

## What Makes GenesisCode Competitive

- Unified semantic stack: language runtime + VCS + package manager + policy + evidence in one model.
- Determinism-first execution: canonical hashes, replay logs, and strict capability routing.
- Agent-operable control plane: JSON contracts, capability audits, workload gauntlets, and release confidence lanes.
- Selfhost-first operations: production CLI defaults to selfhost frontend with explicit no-fallback guardrails.

Known GenesisCode gaps

These are the remaining blockers reflected in `/Users/corbensorenson/Documents/genesisCode/upgrade_plan.md`:
- `P0.2` Remote-registry parity is not closed across native + WASI runtime lanes.
- `P0.3` GPU compute semantics still have planned-family debt before full device-backed confidence.
- `P0.4` Browser/gfx/xr runtime realism families are still planned in coverage audit.
- `P0.5` FFI remains policy-disabled by default with no fully productized safe escalation contract.

Primary evidence paths

- `/Users/corbensorenson/Documents/genesisCode/upgrade_plan.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/CAPABILITY_COVERAGE_STATUS_v0.1.json`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/CAPABILITY_COVERAGE_AUDIT_v0.1.json`
- `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/selfhost_readiness_report.json`
- `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/agent_capability_gauntlet_release_confidence_report.json`
- `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/agent_generative_workloads_report.json`
- `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/gcpm_operation_contract_pack_report.json`
- `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/gpu_device_conformance_report.json`
- `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/gfx_runtime_profile_runtime_report.json`
- `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/webxr_browser_conformance_report.json`
- `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/source_decomposition_progress_report.json`
