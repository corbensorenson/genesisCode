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
| GPU compute (device-backed, deterministic lane) | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Graphics/window/input/audio runtime families | ⚠️ (runtime lanes pass with mixed device/fallback headroom coverage and still depend on host/headroom availability) | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Browser + XR runtime families | ⚠️ (runtime families are implemented but remain host/runtime-availability constrained) | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Host plugin + FFI schemas/contracts | ✅ (deny-by-default default, signed policy opt-in profile enforced for escalation) | ✅ | ⚠️ | ✅ | ✅ | ✅ |
| First-party bridge for network/process/db/crypto/plugin/ffi | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Stage2 CoreForm->WASM translation-validation | ✅ | ✅ | ✅ | ⚠️ | ⚠️ | ✅ |
| Deployment target pipeline in core toolchain | ✅ | ✅ | ✅ | ✅ | ⚠️ | ✅ |
| Assurance profile packs + standards crosswalk | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Tool qualification lineage + evidence closures | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| AI-first modular decomposition + boundary guards | ⚠️ (decomposition/ownership automation is enforced but not yet comprehensive across every module family) | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |

## Capability Family Coverage (from audit)

Source: `docs/spec/CAPABILITY_COVERAGE_AUDIT_v0.1.json`

- Family count: `33`
- Implemented families: `32`
- Planned families: `0`
- Policy-disabled families: `1` (`host/ffi`, deny-by-default)
- Host operations tracked: `187`
- Prelude operations tracked: `184`
- Planned upgrade IDs still open in capability coverage: `none` (all remaining blockers are operational/perf/assurance, not missing capability-family wiring)

Capability-coverage blockers currently preventing "agent can build anything" confidence:
- None in coverage status (open backlog IDs below are operational/perf/assurance blockers).

Implemented capability families (32):
- `browser`: `browser/window`, `browser/input`, `browser/audio`, `browser/storage`
- `core`: `core/store`, `core/refs`, `core/sync`, `core/pkg-low`, `core/vcs-low`, `core/gpk-low`, `core/gc-low`, `core/task`, `core/crypto`, `core/media`
- `editor`: `editor/task`, `editor/watch`, `editor/plugin`, `editor/dialog`, `editor/clipboard`
- `gfx`: `gfx/window`, `gfx/input`, `gfx/audio`, `gfx/time`, `gfx/gpu`, `gfx/xr`
- `gpu`: `gpu/compute`
- `host`: `host/plugin` (with `host/ffi` intentionally policy-disabled by default)
- `io`: `io/fs`, `io/net`, `io/db`
- `sys`: `sys/process`, `sys/time`

## What Makes GenesisCode Competitive

- Unified semantic stack: language runtime + VCS + package manager + policy + evidence in one model.
- Determinism-first execution: canonical hashes, replay logs, and strict capability routing.
- Agent-operable control plane: JSON contracts, capability audits, workload gauntlets, and release confidence lanes.
- Selfhost-first operations: production CLI defaults to selfhost frontend with explicit no-fallback guardrails.

Known GenesisCode gaps

These are the remaining blockers reflected in `/Users/corbensorenson/Documents/genesisCode/upgrade_plan.md`:
- none

Primary evidence paths

- `/Users/corbensorenson/Documents/genesisCode/upgrade_plan.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/CAPABILITY_COVERAGE_STATUS_v0.1.json`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/CAPABILITY_COVERAGE_AUDIT_v0.1.json`
- `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/selfhost_readiness_report.json`
- `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/agent_capability_gauntlet_release_confidence_report.json`
- `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/agent_generative_workloads_report.json`
- `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/gcpm_operation_contract_pack_report.json`
- `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/remote_registry_runtime_parity_report.json`
- `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/gpu_device_conformance_report.json`
- `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/gfx_runtime_profile_runtime_report.json`
- `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/webxr_browser_conformance_report.json`
- `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/gcpm_target_runtime_evidence_report.json`
- `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/source_decomposition_progress_report.json`
- `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/source_decomposition_tracked_parity_report.json`
