# GenesisCode Feature Matrix (Audit Date: 2026-02-26)

Last updated: 2026-02-26  
Scope: capabilities that matter for AI-agent autonomy, selfhost closure, and production runtime trust.

Legend:
- `✅` production-capable and validated in active gates
- `⚠️` available but still materially constrained for “agent can build anything” usage
- `❌` not first-class

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
| Native + WASI + wasm-host runtime surfaces | ⚠️ | ⚠️ | ⚠️ | ✅ | ✅ | ⚠️ |
| Selfhost frontend default in production CLIs | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Full selfhost cutover profile + readiness scorecard | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Strict no-production Rust semantic fallback guard | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| CLI + GCPM JSON schema contracts for agent automation | ✅ | ⚠️ | ⚠️ | ✅ | ⚠️ | ❌ |
| Agent index + skill-pack conformance contracts | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Domain starter registry for agent workflows (27 domains) | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Agent generative workload parity gates (native vs WASI) | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Large-workspace agent iteration SLO lane (>=10k modules) | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Concurrency/task replay stress lane | ✅ | ⚠️ | ✅ | ⚠️ | ⚠️ | ✅ |
| GPU compute capability independent of graphics surface | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Graphics/window/input/audio capability families | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| XR and browser runtime capability families | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| GPU/XR productization conformance lane | ⚠️ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Host plugin + FFI capability schemas | ✅ | ✅ | ⚠️ | ✅ | ✅ | ✅ |
| First-party backend bridge for network/process/db/crypto/plugin/ffi | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Stage2 CoreForm->WASM translation-validation path | ⚠️ | ✅ | ✅ | ⚠️ | ⚠️ | ✅ |
| Deployment target pipeline in core toolchain | ✅ | ✅ | ✅ | ✅ | ⚠️ | ✅ |
| Assurance profile packs + standards crosswalk (DO-178C/NASA/IEC) | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Tool qualification lineage + evidence closures | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| AI-first modular decomposition + boundary guards | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |

## Competitive Positioning

GenesisCode is competitive where other languages require assembling multiple external systems:
- Unified semantic stack: runtime + VCS + package manager + policy + evidence in one model.
- Determinism-first operation: immutable hashes, replay logs, and explicit capability boundaries.
- Agent-operable interfaces: machine-readable command contracts and explicit workflow/perf gates.

Known GenesisCode gaps (tracked in `/Users/corbensorenson/Documents/genesisCode/upgrade_plan.md`):
- `P0.1` Stage2 coverage still rejects supported CoreForm patterns required by deploy targets.
- `P0.2` WASI remote registry parity is incomplete for http(s) remotes.
- `P0.3` GPU device backend lacks full resource lifecycle parity.
- `P0.4` GFX/browser/XR production runtime paths still include simulation/unsupported-op gaps.
- `P1.1` Media conversion support is narrower than production “build anything” needs.
- `P1.2` Editor task runtime remains fixed-task and not fully extensible for agent orchestration.
- `P1.3` Dependency resolver diagnostics/selection depth is insufficient for complex ecosystems.
- `P1.4` Full validation/test iteration time still needs tighter AI inner-loop SLOs.
- `P2.1` Autonomous-repair diagnostics contract is not yet complete across subsystems.
- `P2.2` Capability-family coverage audit and unsupported-op closure is incomplete.
- `P2.3` Untrusted-agent execution safety hardening needs additional enforcement depth.

Primary evidence paths:
- `/Users/corbensorenson/Documents/genesisCode/upgrade_plan.md`
- `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/selfhost_readiness_report.json`
- `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/agent_capability_gauntlet_release_confidence_report.json`
- `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/agent_generative_workloads_report.json`
- `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/large_workspace_agent_perf_report.json`
- `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/gcpm_operation_contract_pack_report.json`
- `/Users/corbensorenson/Documents/genesisCode/crates/gc_opt/src/stage2_wasm.rs`
- `/Users/corbensorenson/Documents/genesisCode/crates/gc_opt/src/stage2_wasm/pipeline_exec.rs`
- `/Users/corbensorenson/Documents/genesisCode/crates/gc_registry/src/registry/client_impl/ping_and_store.rs`
- `/Users/corbensorenson/Documents/genesisCode/crates/gc_registry/src/registry/client_impl/refs.rs`
- `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_gpu_host.rs`
- `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_gpu_device_backend.rs`
- `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_gfx_host.rs`
- `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_browser_host.rs`
- `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_xr_host.rs`
- `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_editor_tasks.rs`
- `/Users/corbensorenson/Documents/genesisCode/crates/gc_effects/src/runner_capability_dispatch/media.rs`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/HOST_ABI.md`
