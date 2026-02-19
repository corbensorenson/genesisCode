# GenesisCode Feature Matrix (Audit Date: 2026-02-19)

Legend:
- `✅` = built-in and first-class
- `⚠️` = partial, optional, or ecosystem-driven
- `❌` = not first-class in the language/toolchain itself

| Capability | GenesisCode | Rust | Go | TypeScript (Node) | Python |
|---|---|---|---|---|---|
| Pure deterministic kernel separated from effects | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Canonical source normalization + stable content hash contract | ✅ | ❌ | ⚠️ | ❌ | ❌ |
| Unforgeable protocol values (sealed EFFECT/ERROR/UNHANDLED) | ✅ | ❌ | ❌ | ❌ | ❌ |
| Deny-by-default capability policy runtime | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Deterministic effect logs + replay checker | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Obligations + evidence artifacts as first-class workflow | ✅ | ❌ | ❌ | ❌ | ❌ |
| Semantic patch artifacts (structural, policy-gated) | ✅ | ❌ | ❌ | ❌ | ❌ |
| Language-native semantic VCS DAG + refs + bundle format | ✅ | ❌ | ❌ | ❌ | ❌ |
| Built-in package/project manager in core CLI surface | ✅ (`gcpm`) | ⚠️ (`cargo`) | ⚠️ (`go mod`) | ⚠️ (`npm/pnpm`) | ⚠️ (`pip/poetry/pixi`) |
| Row-polymorphic contracts + effect rows | ⚠️ (optional stack) | ❌ | ❌ | ❌ | ❌ |
| Optimizer with translation-validation obligation | ⚠️ (conservative subset) | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Selfhost frontend as default runtime path | ✅ | ❌ | ❌ | ❌ | ❌ |
| Fully self-hosted toolchain with zero bootstrap-language dependency | ⚠️ (in progress) | ✅ | ✅ | ⚠️ | ⚠️ |
| WASM pure runtime APIs | ✅ | ⚠️ | ⚠️ | ✅ | ⚠️ |
| WASI CLI surface | ✅ | ⚠️ | ⚠️ | ❌ | ⚠️ |
| Deterministic concurrency/task API with replay semantics | ✅ | ❌ | ❌ | ❌ | ❌ |
| GPU compute + graphics capability surfaces | ⚠️ (capability wrappers; host-backed) | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Supply-chain signing + transparency in primary CLI | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Local artifact GC with reachability roots (refs/locks/pins) | ✅ | ❌ | ❌ | ❌ | ❌ |

Notes:
- This matrix compares first-class language/toolchain behavior, not the total power of third-party ecosystems.
- GenesisCode currently leads on deterministic capability/evidence workflows and semantic VCS/package integration.
- Main remaining gap for GenesisCode is end-to-end self-host completion and hardening, not surface area.

Primary evidence paths:
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/CLI.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/SELF_HOST_BOUNDARY.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/HOST_ABI.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/CONCURRENCY_v0.1.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/OPTIMIZER.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/WASM.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/WASI.md`
