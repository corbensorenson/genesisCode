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
| Fully self-hosted toolchain with zero bootstrap-language dependency | ⚠️ (production binaries are selfhost-first with explicit deterministic artifact recovery from manifest sources; Rust parity remains isolated to dedicated parity harness artifacts) | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Artifact-only bootstrap default across WASM host APIs | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Deterministic concurrency/task API with replay semantics | ✅ | ❌ | ❌ | ❌ | ❌ |
| Multithreaded runtime task execution | ✅ | ✅ | ✅ | ⚠️ | ⚠️ |
| GPU compute + graphics capability surfaces | ✅ (first-class split compute/gfx bundles with explicit cross-over primitives, independent compute-only + gfx-only conformance lanes, and dedicated non-gfx GPU + XR productization kit contracts) | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Media/asset pipeline contracts for AI-generated build lanes | ✅ (first-class `core/media::*` hash/image/audio transcode ops + policy gates + `core/kit/media::*` build contracts) | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Device-backed GPU compute required in release profile | ✅ (policy and lane contracts require `device-runtime`; release/profile health execution now enforces full profile lanes instead of backlog short-circuit, and device conformance remains wired via release lane checks) | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
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
| Deployment/bundle target pipeline in core toolchain | ⚠️ (`gcpm build --target <web|desktop|service|ios|android|edge|service-runtime>` emits deterministic runtime-runner bundles today (`runtime_contract/boot/smoke`); platform-native executable packaging/signing lanes remain open in P0.3) | ⚠️ | ✅ | ⚠️ | ⚠️ |
| Workspace semantic graph/refactor API for automation | ✅ (`semantic-edit workspace-graph` + `semantic-edit refactor-plan` + `semantic-edit apply-plan` provide deterministic dependency graph export, machine-mergeable patch planning, and obligation-gated plan application with conflict diagnostics) | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Machine-consumable agent authoring contract | ✅ (`docs/spec/WRITE_GENESISCODE_SKILL_v0.1.json` + `docs/spec/WRITE_GENESISCODE_SKILL_PACK_v0.1.json` + canonical handbook `docs/write_genesisCode_skill.md` + executable conformance gates `scripts/check_genesiscode_authoring_skill.sh`, `scripts/check_write_genesiscode_skill_pack.sh`, `scripts/check_write_genesiscode_skill_guide.sh`, `scripts/check_write_genesiscode_skill_distribution.sh`, and `scripts/check_write_genesiscode_skill_conformance.sh`) | ❌ | ❌ | ❌ | ❌ |
| Supply-chain signing + transparency in primary CLI | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Local artifact GC by refs/locks/pins reachability | ✅ | ❌ | ❌ | ❌ | ❌ |
| Runtime backend profile selection through project manager workflows | ✅ | ✅ | ✅ | ⚠️ | ⚠️ |
| Deterministic non-gfx runtime profiling in core workflow | ✅ (`gcpm profile-runtime` emits task/IO/memory profile artifacts with history-aware p95 regression gates) | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Generative workload regression gates with enforced historical baselines | ✅ (`agent_generative_workloads*` lanes are fail-closed with seeded baseline histories, per-case minimum-history enforcement, and active regression budgets in native/parity runs) | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Enforced runtime wall-time budgets for strict/full profile lanes | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Perf/hot-path gate operability under constrained local disk headroom | ✅ (shared strict-mode + heavy-gate preflight contracts and dedicated `gpu/gfx` low-headroom conformance lanes are enforced in strict profiles) | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Health gate lock-aware cargo scheduling + shared build cache target | ✅ (`check_upgrade_plan_health.sh` partitions cargo/non-cargo gates, shares `CARGO_TARGET_DIR`, and supports profile-scoped cargo warmup orchestration) | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Bidirectional requirements traceability (system/HLR/LLR -> code -> tests -> artifact) | ✅ (`gcpm trace` + `:requirements-trace` schema + fail-closed policy gates on refs/publish/registry) | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Structural coverage profiles (decision/MC/DC) | ✅ (`core/obligation::coverage-decision` + `core/obligation::coverage-mcdc` with fail-closed gates + structural evidence payloads) | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Qualified-tool evidence bundles for regulated release | ✅ (`gcpm qualify` enforces snapshot-bound run-manifest lineage from local store (`--test-artifact id=run-manifest-hex64`), validates canonical manifest/artifact integrity, and fails closed on lineage/policy/profile mismatches) | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Independent verifier role-separation policy enforcement | ✅ (ref/publish policy classes support required roles + per-role minimums + independence pairs enforced on valid attestations) | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| Standards-oriented assurance profile packs (DO-178C/NASA/IEC) | ⚠️ (`gcpm assurance-pack` profile lanes and deterministic crosswalk contracts exist, but high-assurance object-equivalence and independent-verifier closure remain open in P2.2) | ⚠️ | ⚠️ | ⚠️ | ⚠️ |

Notes:
- This compares first-class language/toolchain semantics, not total ecosystem power.
- GenesisCode is strongest on deterministic capability/evidence workflows and semantic VCS/pkg integration.
- Red-team backlog currently contains active P0/P1 blockers; see `upgrade_plan.md`.
- Regulated-standard alignment status below is an engineering-readiness view, not a formal certification claim.

Regulated assurance readiness snapshot (indicative):
- `DO-178C DAL A/B`: ⚠️ partial alignment (requirements traceability, structural decision/MC/DC coverage, tool qualification workflows, and deterministic assurance-pack bundles are in place; formal certification program execution remains external to the language runtime/toolchain).
- `NASA NPR 7150.2 Class A/B`: ⚠️ partial alignment (deterministic runtime, traceability artifacts, role gates, structural coverage, and assurance-pack bundling are in place; independent mission IV&V process controls remain organizational responsibilities).
- `IEC 62304 Class C`: ⚠️ partial alignment (lifecycle evidence/policy gates, qualification artifacts, and reproducible assurance-pack bundles are in place; full device-risk process qualification remains product-program specific).

Known GenesisCode gaps:
- `P0.3`: `gcpm build --target` emits runtime-runner contracts, not platform-native executable artifacts.
- `P1.3`: `gcpm` dependency resolution remains local-only v0.1 without semver/range solving.
- `P2.2`: Assurance packs require stronger object/lineage/independence closure for high-assurance programs.

Primary evidence paths:
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/CLI.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/FEATURE_MATRIX_EVIDENCE_v0.1.json`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/FEATURE_MATRIX_EVIDENCE_v0.1.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/ASSURANCE_ARTIFACTS_v0.1.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/ASSURANCE_PROFILE_PACKS_v0.1.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/ASSURANCE_STANDARDS_CROSSWALK_v0.1.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/ASSURANCE_STANDARDS_CROSSWALK_v0.1.json`
- `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/selfhost_readiness_report.json`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/SELF_HOST_BOUNDARY.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/HOST_ABI.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/CONCURRENCY_v0.1.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/DOMAIN_KITS_v0.1.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/GFX_CAPS.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/GPU_COMPUTE_RUNTIME_PROFILE_v0.1.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/TEST_EXECUTION_PROFILES_v0.1.md`
- `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/doc_complexity_report.json`
- `/Users/corbensorenson/Documents/genesisCode/.genesis/perf/source_decomposition_progress_report.json`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/AGENT_CAPABILITY_GAUNTLET_v0.1.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/AGENT_WORKFLOW_RUNTIME_PARITY_v0.1.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/WASI.md`
- `/Users/corbensorenson/Documents/genesisCode/docs/spec/WASM.md`
- `/Users/corbensorenson/Documents/genesisCode/upgrade_plan.md`
