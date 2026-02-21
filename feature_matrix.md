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
| Fully self-hosted toolchain with zero bootstrap-language dependency | ✅ (production binaries are selfhost-first; Rust parity is isolated to dedicated parity harness artifacts) | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Artifact-only bootstrap default across WASM host APIs | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Deterministic concurrency/task API with replay semantics | ✅ | ❌ | ❌ | ❌ | ❌ |
| Multithreaded runtime task execution | ✅ | ✅ | ✅ | ⚠️ | ⚠️ |
| GPU compute + graphics capability surfaces | ⚠️ (implemented, feature/profile-gated) | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Device-backed GPU compute required in release profile | ✅ (`release-full` health profile + dedicated GPU conformance lane require `device-runtime`) | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Network + process execution as policy-gated capabilities | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Filesystem management capability surface (`stat/list/mkdir/rename/remove`) | ✅ | ✅ | ✅ | ✅ | ✅ |
| Process lifecycle + stdio streaming primitives | ✅ | ✅ | ✅ | ✅ | ✅ |
| Raw socket/stream networking primitives | ✅ (`io/net::tcp-*`, `io/net::udp-*`, `io/net::dns-resolve`, `io/net::ws-*`, `io/net::http-request`) | ⚠️ | ✅ | ⚠️ | ⚠️ |
| Generic host extension/FFI capability ABI | ✅ (`host/plugin::command` + `editor/plugin::command` wrapper) | ✅ | ⚠️ | ⚠️ | ⚠️ |
| WASM runtime APIs | ✅ | ✅ | ⚠️ | ✅ | ⚠️ |
| WASI CLI support | ✅ | ✅ | ⚠️ | ❌ | ⚠️ |
| Schema-stable JSON CLI contracts for agents | ✅ | ⚠️ | ❌ | ❌ | ❌ |
| Supply-chain signing + transparency in primary CLI | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Local artifact GC by refs/locks/pins reachability | ✅ | ❌ | ❌ | ❌ | ❌ |
| Runtime backend profile selection through project manager workflows | ✅ | ✅ | ✅ | ⚠️ | ⚠️ |
| Enforced runtime wall-time budgets for strict/full profile lanes | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Bidirectional requirements traceability (system/HLR/LLR -> code -> tests -> artifact) | ✅ (`gcpm trace` + `:requirements-trace` schema + fail-closed policy gates on refs/publish/registry) | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Structural coverage profiles (decision/MC/DC) | ✅ (`core/obligation::coverage-decision` + `core/obligation::coverage-mcdc` with fail-closed gates + structural evidence payloads) | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Qualified-tool evidence bundles for regulated release | ✅ (`gcpm qualify` + `:tool-qualification` schema + fail-closed policy gates) | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Independent verifier role-separation policy enforcement | ✅ (ref/publish policy classes support required roles + per-role minimums + independence pairs enforced on valid attestations) | ⚠️ | ⚠️ | ⚠️ | ⚠️ |

Notes:
- This compares first-class language/toolchain semantics, not total ecosystem power.
- GenesisCode is strongest on deterministic capability/evidence workflows and semantic VCS/pkg integration.
- Current main GenesisCode gaps are concentrated in non-`P0`/`P1` hardening and performance tuning lanes.
- Regulated-standard alignment status below is an engineering-readiness view, not a formal certification claim.

Regulated assurance readiness snapshot (indicative):
- `DO-178C DAL A/B`: ⚠️ partial alignment (requirements traceability, structural decision/MC/DC coverage, tool qualification workflows, and deterministic assurance-pack bundles are in place; formal certification program execution remains external to the language runtime/toolchain).
- `NASA NPR 7150.2 Class A/B`: ⚠️ partial alignment (deterministic runtime, traceability artifacts, role gates, structural coverage, and assurance-pack bundling are in place; independent mission IV&V process controls remain organizational responsibilities).
- `IEC 62304 Class C`: ⚠️ partial alignment (lifecycle evidence/policy gates, qualification artifacts, and reproducible assurance-pack bundles are in place; full device-risk process qualification remains product-program specific).

Known GenesisCode gaps (current red-team focus):
- `P1.2` Iteration profile budgets still permit slow loops (`changed-fast` up to 5m, `full-cross-host` up to 20m).
- `P1.3` Spec/doc surface is large (91 Markdown docs total; 70 in `docs/spec`) and needs stronger bundle-first consolidation.
- `P2.1` High-level domain kits are limited; prelude breadth is still concentrated in core/gfx/editor layers.
- `P2.2` Device-runtime GPU conformance is anchored to a single CI host class.

Primary evidence paths:
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/CLI.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/ASSURANCE_ARTIFACTS_v0.1.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/SELF_HOST_BOUNDARY.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/HOST_ABI.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/CONCURRENCY_v0.1.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/GFX_CAPS.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/GPU_COMPUTE_RUNTIME_PROFILE_v0.1.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/TEST_EXECUTION_PROFILES_v0.1.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/WASI.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/WASM.md`
- `/Users/corbensorenson/Documents/genesisCode/upgrade_plan.md`
