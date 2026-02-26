# GenesisCode Feature Matrix (Audit Date: 2026-02-26)

Last updated: 2026-02-26  
Scope: capabilities that matter for AI-agent autonomy, selfhost closure, and production-grade runtime trust.

Legend:
- `✅` production-capable and validated in active gates
- `⚠️` available with material remaining closure/hardening work
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
| Native + WASI + wasm-host runtime surfaces | ✅ | ⚠️ | ⚠️ | ✅ | ✅ | ⚠️ |
| Selfhost frontend default in production CLIs | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Full selfhost cutover profile + readiness scorecard | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Strict no-production Rust semantic fallback guard | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| CLI + GCPM JSON schema contracts for agent automation | ✅ | ⚠️ | ⚠️ | ✅ | ⚠️ | ❌ |
| Agent index + skill-pack conformance contracts | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Domain starter registry for agent workflows (27 domains) | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Agent generative workload parity gates (native vs WASI) | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Large-workspace agent iteration SLO lane (>=10k modules) | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Concurrency/task replay stress lane | ✅ | ⚠️ | ✅ | ⚠️ | ⚠️ | ✅ |
| GPU compute capability independent of graphics surface | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Graphics/window/input/audio capability families | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| XR and browser runtime capability families | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| GPU/XR productization conformance lane | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Host plugin + FFI capability schemas | ✅ | ✅ | ⚠️ | ✅ | ✅ | ✅ |
| First-party backend bridge for network/process/db/crypto/plugin/ffi | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Stage2 CoreForm->WASM translation-validation path | ✅ | ✅ | ✅ | ⚠️ | ⚠️ | ✅ |
| Deployment target pipeline in core toolchain | ✅ | ✅ | ✅ | ✅ | ⚠️ | ✅ |
| Assurance profile packs + standards crosswalk (DO-178C/NASA/IEC) | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Tool qualification lineage + evidence closures | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| AI-first modular decomposition + boundary guards | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |

## Competitive Positioning

- GenesisCode’s differentiator is integrated semantics: language runtime, semantic VCS, package manager, obligation/evidence pipeline, and deterministic replay model are one contract surface.
- It is stronger than mainstream stacks on agent verifiability (machine-readable CLI/GCPM reports, reproducible hashes, replay logs, policy gates, and assurance closures).
- It is weaker today on some “build anything immediately” host/runtime edges where first-party backends still expose partial or placeholder semantics (tracked below).

Known GenesisCode gaps identified in this audit (tracked in `/Users/corbensorenson/Documents/genesisCode/upgrade_plan.md`):
- `P1.4` - agent-capability-gauntlet release-confidence lane is still `ok=false` (`workflow-failures`, `domain-coverage`).

Primary evidence paths:
- `/Users/corbensorenson/Documents/genesisCode/upgrade_plan.md`
- `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/selfhost_readiness_report.json`
- `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/agent_capability_gauntlet_release_confidence_report.json`
- `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/agent_generative_workloads_report.json`
- `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/large_workspace_agent_perf_report.json`
- `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/domain_starter_registry_bootstrap_report.json`
- `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/gcpm_operation_contract_pack_report.json`
- `/Users/corbensorenson/Documents/genesisCode/crates/gc_opt/src/stage2_wasm.rs`
- `/Users/corbensorenson/Documents/genesisCode/crates/gc_opt/src/stage2_wasm/pipeline_exec.rs`
- `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/host_bridge_runtime.rs`
- `/Users/corbensorenson/Documents/genesisCode/crates/gc_cli_driver/src/pkg_workspace_ops_build_artifacts.rs`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/HOST_ABI.md`
