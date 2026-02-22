# GenesisCode Feature Matrix (Audit Date: 2026-02-22)

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
| Fully self-hosted toolchain with zero bootstrap-language dependency | ⚠️ (production binaries are selfhost-first, but artifact regeneration/recovery still requires explicit bootstrap-seed handling; Rust parity remains isolated to dedicated parity harness artifacts) | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Artifact-only bootstrap default across WASM host APIs | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Deterministic concurrency/task API with replay semantics | ✅ | ❌ | ❌ | ❌ | ❌ |
| Multithreaded runtime task execution | ✅ | ✅ | ✅ | ⚠️ | ⚠️ |
| GPU compute + graphics capability surfaces | ⚠️ (implemented, feature/profile-gated) | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Media/asset pipeline contracts for AI-generated build lanes | ✅ (first-class `core/media::*` hash/image/audio transcode ops + policy gates + `core/kit/media::*` build contracts) | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Device-backed GPU compute required in release profile | ✅ (`release-full` health profile and agent gauntlet release posture require `device-runtime`; CI includes multi-vendor matrix gate for `nvidia-linux`, `amd-linux`, `intel-windows`, `apple-macos` with adapter-scoped artifact retention) | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Network + process execution as policy-gated capabilities | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Filesystem management capability surface (`stat/list/mkdir/rename/remove`) | ✅ (first-class `core/fs::*` wrappers + required gauntlet domain coverage) | ✅ | ✅ | ✅ | ✅ |
| Process lifecycle + stdio streaming primitives | ✅ (first-class `core/process::*` wrappers + required gauntlet domain coverage) | ✅ | ✅ | ✅ | ✅ |
| Raw socket/stream networking primitives | ✅ (first-class `core/net::*` socket wrappers + required gauntlet domain coverage) | ⚠️ | ✅ | ⚠️ | ⚠️ |
| Inbound server networking primitives (listen/accept/http-serve/ws-accept) | ✅ (first-class `core/net::*` inbound listener/accept/respond wrappers + policy-gated bind/request-size controls + gauntlet domain coverage) | ⚠️ | ✅ | ✅ | ✅ |
| Generic host extension/FFI capability ABI | ✅ (first-class `core/plugin::*` wrappers with typed request/response schema ids, runtime schema validation, and policy allowlists) | ✅ | ⚠️ | ⚠️ | ⚠️ |
| Plugin command surface hardening (command allowlists + bridge digest pinning) | ✅ (`host/plugin::command` and `editor/plugin::command` require `allow_commands`; bridge transport requires `bridge_cmd_sha256` and fails closed when missing/mismatched) | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Browser runtime host profile for wasm-hosted apps | ✅ (first-party `browser/window::*`, `browser/input::*`, `browser/audio::*`, `browser/storage::*` families + `first_party_profile=\"browser\"` for gfx window/input/audio parity lanes) | ⚠️ | ⚠️ | ✅ | ⚠️ |
| WebXR runtime primitives (session/frame/input/haptics) | ✅ (first-class `gfx/xr::*` session/frame/input/haptics/submit/close contracts across first-party + `xr_backend=\"webxr-device\"` deterministic bridge envelopes + browser-native conformance lane with deterministic capture/replay hashes) | ⚠️ | ⚠️ | ✅ | ⚠️ |
| Advanced XR spatial primitives (anchors/hands/mesh/layers) | ✅ (first-class `gfx/xr::*` anchors/hands/hit-test/spatial-mesh/layer lifecycle contracts with deterministic envelopes and policy gates) | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Durable data capability family (`io/db::*`) | ✅ (first-class SQL + KV bridge-backed contracts with policy-gated target/query/row/byte bounds and replay-stable envelopes) | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| First-class cryptography capability family | ✅ (first-class `core/crypto::*` hash/sign/verify/KDF/AEAD capability contracts with policy-gated bridge envelopes) | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| WASM runtime APIs | ✅ | ✅ | ⚠️ | ✅ | ⚠️ |
| WASI CLI support | ✅ | ✅ | ⚠️ | ❌ | ⚠️ |
| Schema-stable JSON CLI contracts for agents | ✅ | ⚠️ | ❌ | ❌ | ❌ |
| Deployment/bundle target pipeline in core toolchain | ✅ (`gcpm build --target <web|desktop|service>` emits immutable deterministic bundles with build-manifest + provenance contracts) | ⚠️ | ✅ | ⚠️ | ⚠️ |
| Workspace semantic graph/refactor API for automation | ✅ (`semantic-edit workspace-graph` + `semantic-edit refactor-plan` provide deterministic dependency graph export and machine-mergeable multi-file patch planning) | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Machine-consumable agent authoring contract | ✅ (`docs/spec/WRITE_GENESISCODE_SKILL_v0.1.json` + `docs/spec/WRITE_GENESISCODE_SKILL_PACK_v0.1.json` + conformance gates `scripts/check_genesiscode_authoring_skill.sh` and `scripts/check_write_genesiscode_skill_pack.sh`) | ❌ | ❌ | ❌ | ❌ |
| Supply-chain signing + transparency in primary CLI | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Local artifact GC by refs/locks/pins reachability | ✅ | ❌ | ❌ | ❌ | ❌ |
| Runtime backend profile selection through project manager workflows | ✅ | ✅ | ✅ | ⚠️ | ⚠️ |
| Deterministic non-gfx runtime profiling in core workflow | ✅ (`gcpm profile-runtime` emits task/IO/memory profile artifacts with history-aware p95 regression gates) | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Generative workload regression gates with enforced historical baselines | ✅ (`agent_generative_workloads*` lanes are fail-closed with seeded baseline histories, per-case minimum-history enforcement, and active regression budgets in native/parity runs) | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Enforced runtime wall-time budgets for strict/full profile lanes | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Perf/hot-path gate operability under constrained local disk headroom | ✅ (shared `GENESIS_PERF_DISK_STRICT_MODE=auto|1|0`; default `auto` keeps CI fail-closed while avoiding local precheck false negatives) | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Health gate lock-aware cargo scheduling + shared build cache target | ✅ (`check_upgrade_plan_health.sh` partitions cargo/non-cargo gates, shares `CARGO_TARGET_DIR`, and supports profile-scoped cargo warmup orchestration) | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Bidirectional requirements traceability (system/HLR/LLR -> code -> tests -> artifact) | ✅ (`gcpm trace` + `:requirements-trace` schema + fail-closed policy gates on refs/publish/registry) | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Structural coverage profiles (decision/MC/DC) | ✅ (`core/obligation::coverage-decision` + `core/obligation::coverage-mcdc` with fail-closed gates + structural evidence payloads) | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Qualified-tool evidence bundles for regulated release | ✅ (`gcpm qualify` + `:tool-qualification` schema + fail-closed policy gates) | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Independent verifier role-separation policy enforcement | ✅ (ref/publish policy classes support required roles + per-role minimums + independence pairs enforced on valid attestations) | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Standards-oriented assurance profile packs (DO-178C/NASA/IEC) | ✅ (`gcpm assurance-pack` profile lanes + `policies/assurance/profile_packs.toml` + deterministic crosswalk contract) | ⚠️ | ⚠️ | ⚠️ | ⚠️ |

Notes:
- This compares first-class language/toolchain semantics, not total ecosystem power.
- GenesisCode is strongest on deterministic capability/evidence workflows and semantic VCS/pkg integration.
- Red-team backlog currently contains active P0/P1 blockers; see `upgrade_plan.md`.
- Regulated-standard alignment status below is an engineering-readiness view, not a formal certification claim.

Regulated assurance readiness snapshot (indicative):
- `DO-178C DAL A/B`: ⚠️ partial alignment (requirements traceability, structural decision/MC/DC coverage, tool qualification workflows, and deterministic assurance-pack bundles are in place; formal certification program execution remains external to the language runtime/toolchain).
- `NASA NPR 7150.2 Class A/B`: ⚠️ partial alignment (deterministic runtime, traceability artifacts, role gates, structural coverage, and assurance-pack bundling are in place; independent mission IV&V process controls remain organizational responsibilities).
- `IEC 62304 Class C`: ⚠️ partial alignment (lifecycle evidence/policy gates, qualification artifacts, and reproducible assurance-pack bundles are in place; full device-risk process qualification remains product-program specific).

Known GenesisCode gaps (current red-team focus):
- P0.1 Artifact bootstrap deadlock in production selfhost flow.
- P1.1 `dev-fast` wall-time remains too high for tight agent loops.
- P1.2 Build cache/target-dir sprawl still risks ENOSPC under repeated gate runs.
- P1.3 Runtime backend feature matrix check lacks enforced wall-time SLO.
- P1.4 Production CLI parse/help surface checks remain high-latency release runs.
- P1.5 GPU compute backend fallback defaults are too permissive for strict release lanes.
- P1.6 Agent authoring skill lacks executable generation-quality conformance gates.
- P1.7 Bootstrap-independence capability claim is ahead of current recovery behavior.
- P2.1 `gcpm build` targets are limited to `web|desktop|service`.
- P2.2 GPU compute and graphics stacks need clearer architectural decoupling.
- P2.3 Remaining high-churn authoring modules still need deeper decomposition.
- P2.4 `write_genesisCode_skill` needs executable v1 distribution for multi-agent use.

Primary evidence paths:
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/CLI.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/FEATURE_MATRIX_EVIDENCE_v0.1.json`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/FEATURE_MATRIX_EVIDENCE_v0.1.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/ASSURANCE_ARTIFACTS_v0.1.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/ASSURANCE_PROFILE_PACKS_v0.1.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/ASSURANCE_STANDARDS_CROSSWALK_v0.1.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/ASSURANCE_STANDARDS_CROSSWALK_v0.1.json`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/SELF_HOST_BOUNDARY.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/HOST_ABI.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/CONCURRENCY_v0.1.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/DOMAIN_KITS_v0.1.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/GFX_CAPS.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/GPU_COMPUTE_RUNTIME_PROFILE_v0.1.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/TEST_EXECUTION_PROFILES_v0.1.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/AGENT_CAPABILITY_GAUNTLET_v0.1.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/AGENT_WORKFLOW_RUNTIME_PARITY_v0.1.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/WASI.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/WASM.md`
- `/Users/corbensorenson/Documents/genesisCode/upgrade_plan.md`
