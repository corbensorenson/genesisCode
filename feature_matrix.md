# GenesisCode Feature Matrix (Audit Date: 2026-02-21)

Legend:
- `✅` = first-class and built into the primary language/toolchain surface
- `⚠️` = partial, optional, profile-gated, or primarily ecosystem-driven
- `❌` = not first-class in the primary language/toolchain itself

| Capability | GenesisCode | Rust | Go | TypeScript (Node) | Python |
|---|---|---|---|---|---|
| Pure deterministic kernel separated from effects | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Canonical CoreForm normalization + stable content hashing contract | ✅ | ❌ | ⚠️ | ❌ | ❌ |
| Unforgeable protocol values (sealed UNHANDLED/EFFECT/ERROR) | ✅ | ❌ | ❌ | ❌ | ❌ |
| Deny-by-default capability policy runtime | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Deterministic effect logs + replay checker | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Obligations + evidence artifacts in core workflow | ✅ | ❌ | ❌ | ❌ | ❌ |
| Language-native semantic VCS DAG + refs + bundles | ✅ | ❌ | ❌ | ❌ | ❌ |
| Built-in package/project manager | ✅ (`gcpm/pkg`) | ✅ (`cargo`) | ✅ (`go mod`) | ⚠️ (`npm/pnpm/yarn`) | ⚠️ (`pip/poetry/pixi`) |
| Strict selfhost frontend default in production CLI | ✅ | ❌ | ❌ | ❌ | ❌ |
| Explicit selfhost-only execution mode | ✅ | ❌ | ❌ | ❌ | ❌ |
| Fully self-hosted toolchain with zero bootstrap-language dependency | ⚠️ (close; parity/bootstrap surfaces remain) | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Artifact-only bootstrap default across WASM host APIs | ⚠️ (native/WASI strict; wasm APIs still expose embedded path defaults) | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Deterministic concurrency/task API with replay semantics | ✅ | ❌ | ❌ | ❌ | ❌ |
| Multithreaded runtime task execution | ✅ | ✅ | ✅ | ⚠️ | ⚠️ |
| GPU compute + graphics capability surfaces | ⚠️ (implemented, feature/profile-gated) | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Device-backed GPU compute required in release profile | ⚠️ (device lane exists but fallback remains accepted by default) | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Network + process execution as policy-gated capabilities | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Raw socket/stream networking primitives | ❌ (HTTP request only) | ⚠️ | ✅ | ⚠️ | ⚠️ |
| Generic host extension/FFI capability ABI | ⚠️ (editor-scoped plugin bridge only) | ✅ | ⚠️ | ⚠️ | ⚠️ |
| WASM runtime APIs | ✅ | ✅ | ⚠️ | ✅ | ⚠️ |
| WASI CLI support | ✅ | ✅ | ⚠️ | ❌ | ⚠️ |
| Schema-stable JSON CLI contracts for agents | ✅ | ⚠️ | ❌ | ❌ | ❌ |
| Supply-chain signing + transparency in primary CLI | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Local artifact GC by refs/locks/pins reachability | ✅ | ❌ | ❌ | ❌ | ❌ |
| Runtime backend profile selection through project manager workflows | ⚠️ (build profiles exist; gcpm runtime profile control is incomplete) | ✅ | ✅ | ⚠️ | ⚠️ |
| Enforced runtime wall-time budgets for strict/full profile lanes | ⚠️ (fast-loop budgets enforced; strict/full lanes docs-first) | ⚠️ | ⚠️ | ⚠️ | ⚠️ |

Notes:
- This compares first-class language/toolchain semantics, not total ecosystem power.
- GenesisCode is strongest on deterministic capability/evidence workflows and semantic VCS/pkg integration.
- Current main GenesisCode gaps are no longer P0/P1 structural blockers; remaining work is primarily performance, ergonomics, and capability hardening.

Known GenesisCode gaps (current red-team focus):
- `P0.1` Artifact-only bootstrap is not yet enforced as the default for production WASM selfhost APIs.
- `P1.1` Network capability surface is HTTP-only and lacks first-class stream/socket primitives.
- `P1.2` Generic host extension/FFI ABI is incomplete beyond editor-scoped plugin bridge ops.
- `P1.3` Runtime backend profile selection is not yet first-class in `gcpm` project workflows.
- `P1.4` Release profile still permits deterministic GPU fallback instead of requiring device-runtime conformance.
- `P1.5` Strict/full profile runtime budgets are not yet enforced via measured wall-time regression gates.
- `P1.6` High-churn Rust and `.gc` hotspots still need further decomposition for AI edit reliability.

Primary evidence paths:
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/CLI.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/SELF_HOST_BOUNDARY.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/HOST_ABI.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/CONCURRENCY_v0.1.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/GFX_CAPS.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/GPU_COMPUTE_RUNTIME_PROFILE_v0.1.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/TEST_EXECUTION_PROFILES_v0.1.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/WASI.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/WASM.md`
- `/Users/corbensorenson/Documents/genesisCode/upgrade_plan.md`
