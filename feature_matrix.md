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
| Deterministic concurrency/task API with replay semantics | ✅ | ❌ | ❌ | ❌ | ❌ |
| Multithreaded runtime task execution | ✅ | ✅ | ✅ | ⚠️ | ⚠️ |
| GPU compute + graphics capability surfaces | ⚠️ (implemented, feature/profile-gated) | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Network + process execution as policy-gated capabilities | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| WASM runtime APIs | ✅ | ✅ | ⚠️ | ✅ | ⚠️ |
| WASI CLI support | ✅ | ✅ | ⚠️ | ❌ | ⚠️ |
| Schema-stable JSON CLI contracts for agents | ✅ | ⚠️ | ❌ | ❌ | ❌ |
| Supply-chain signing + transparency in primary CLI | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Local artifact GC by refs/locks/pins reachability | ✅ | ❌ | ❌ | ❌ | ❌ |

Notes:
- This compares first-class language/toolchain semantics, not total ecosystem power.
- GenesisCode is strongest on deterministic capability/evidence workflows and semantic VCS/pkg integration.
- Current main GenesisCode gaps are runtime hardening and packaging consistency, not missing surface area.

Known GenesisCode gaps (current red-team focus):
- Backend-capable production feature wiring (`gpu-device-backend`, `gfx-desktop-backend`) is not yet standardized in release builds.
- `device-runtime` backend currently covers submit/introspection lanes, not full canonical GPU lifecycle.
- GPU backend naming is split across runtime and microbench surfaces (`device-runtime` vs `device-bridge`).

Primary evidence paths:
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/CLI.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/SELF_HOST_BOUNDARY.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/HOST_ABI.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/CONCURRENCY_v0.1.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/GFX_CAPS.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/GPU_COMPUTE_RUNTIME_PROFILE_v0.1.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/WASI.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/WASM.md`
- `/Users/corbensorenson/Documents/genesisCode/upgrade_plan.md`
